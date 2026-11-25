//! The Add repository screen: search GitHub or GitLab, paste a URL, or pick a local
//! folder. Searches, clones and the folder check run in background tasks; `view` only
//! reads the panel's state.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use colony_ui::theme::Palette;
use colony_ui::Typography;
use iced::futures::{SinkExt, Stream};
use iced::widget::Column;
use iced::widget::{button, column, container, progress_bar, row, scrollable, text, text_input};
use iced::{Alignment, Border, Element, Length, Task};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use reqwest::Client;
use reqwest::StatusCode;
use serde::Deserialize;
use url::Url;

use crate::auth::AuthManager;
use crate::config::NetworkOptions;
use crate::error::AppError;
use crate::git::clone::{clone_repository, CloneProgress, CloneRequest};
use crate::i18n::{t, tf};
use crate::ui::changes::{field, horizontal_divider};
use crate::ui::context::RepoContext;
use crate::ui::history::{avatar, avatar_accent, initials, relative_time};
use crate::ui::widgets::button as gs_button;
use crate::ui::widgets::chip::{chip, Tone};
use crate::ui::widgets::segmented::segmented;

/// Where the repository comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    GitHub,
    GitLab,
    Url,
    LocalFolder,
}

impl Source {
    const ALL: [Source; 4] = [
        Source::GitHub,
        Source::GitLab,
        Source::Url,
        Source::LocalFolder,
    ];

    fn label(self) -> &'static str {
        match self {
            Source::GitHub => t("add_repo_github"),
            Source::GitLab => t("add_repo_gitlab"),
            Source::Url => t("add_repo_url"),
            Source::LocalFolder => t("add_repo_local_folder"),
        }
    }

    /// The host searched, for the sources that search one.
    fn host(self) -> Option<&'static str> {
        match self {
            Source::GitHub => Some("github.com"),
            Source::GitLab => Some("gitlab.com"),
            Source::Url | Source::LocalFolder => None,
        }
    }
}

/// A repository found by a search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRepo {
    /// "owner/name" on GitHub, "Group / Name" on GitLab.
    pub name: String,
    pub url: String,
    pub description: Option<String>,
    /// Unknown for GitLab, whose short project listing leaves it out.
    pub private: Option<bool>,
    pub stars: u64,
    /// Last update, in seconds since the epoch.
    pub updated: Option<i64>,
}

impl RemoteRepo {
    /// The owner and the repository name, split at the last "/".
    fn owner_and_name(&self) -> (&str, &str) {
        match self.name.rsplit_once('/') {
            Some((owner, name)) => (owner.trim(), name.trim()),
            None => ("", self.name.as_str()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    SelectSource(Source),
    QueryChanged(String),
    /// Searches now, without waiting for the typing pause.
    Search,
    /// The typing pause of search `version` is over.
    DebouncedSearch(u32),
    SearchResult(u32, Result<Vec<RemoteRepo>, AppError>),
    SelectRepo(usize),
    UrlChanged(String),
    DestinationChanged(String),
    ChooseDestination,
    DestinationChosen(Option<String>),
    StartClone,
    CloneProgress(CloneProgress),
    CloneResult(Result<(), AppError>),
    /// A display frame while the progress bar eases toward the clone's progress.
    Frame,
    RetryClone,
    ChooseFolder,
    FolderChosen(Option<String>),
    /// The picked folder, and whether it holds a repository.
    FolderChecked(PathBuf, bool),
    /// Opens Preferences → Accounts; `app.rs` routes it.
    OpenAccounts,
}

pub struct AddRepoPanel {
    source: Source,
    query: String,
    /// The URL typed for the URL source.
    url: String,
    destination: String,
    /// The folder the repository name is appended to: the default clone folder, or the
    /// one picked with Browse.
    base_destination: PathBuf,
    results: Vec<RemoteRepo>,
    selected: Option<usize>,
    search_status: Option<String>,
    progress: Option<CloneProgress>,
    /// The ratio the bar shows while it eases toward the clone's progress.
    shown_ratio: f32,
    /// Bytes per second since the clone started.
    speed: f64,
    started: Option<Instant>,
    cloning: bool,
    active_destination: Option<PathBuf>,
    last_request: Option<CloneRequest>,
    network: NetworkOptions,
    /// Each keystroke, search or source change bumps it; only the current version's pause
    /// and result are applied.
    search_version: u32,
    checking_folder: bool,
    folder_error: Option<String>,
}

impl AddRepoPanel {
    pub fn new(default_destination: String, network: NetworkOptions) -> Self {
        Self {
            source: Source::GitHub,
            query: String::new(),
            url: String::new(),
            base_destination: PathBuf::from(&default_destination),
            destination: default_destination,
            results: Vec::new(),
            selected: None,
            search_status: None,
            progress: None,
            shown_ratio: 0.0,
            speed: 0.0,
            started: None,
            cloning: false,
            active_destination: None,
            last_request: None,
            network,
            search_version: 0,
            checking_folder: false,
            folder_error: None,
        }
    }

    /// Whether `version` is the search in progress, not one it replaced.
    pub fn is_current_search(&self, version: u32) -> bool {
        version == self.search_version
    }

    pub fn set_default_destination(&mut self, destination: String) {
        self.base_destination = PathBuf::from(destination);
        self.refresh_destination();
    }

    /// Whether the progress bar still has to ease toward the clone's progress.
    pub fn is_easing(&self) -> bool {
        self.cloning
            && colony_ui::effects_enabled()
            && self.shown_ratio != progress_ratio(self.progress.as_ref())
    }

    pub fn set_network_preferences(&mut self, network: NetworkOptions) {
        self.network = network;
    }

    /// Handles `message`; the path is set once a repository was cloned or a folder opened.
    pub fn update(
        &mut self,
        message: Message,
        auth: &AuthManager,
    ) -> (Task<Message>, Option<PathBuf>) {
        match message {
            Message::SelectSource(source) => {
                self.source = source;
                self.results.clear();
                self.selected = None;
                self.search_status = None;
                self.folder_error = None;
                // Drops the search still running on the previous source.
                self.search_version += 1;
                self.refresh_destination();
            }
            Message::QueryChanged(query) => {
                self.query = query;
                self.search_status = None;
                self.search_version += 1;
                let version = self.search_version;
                let pause = Task::perform(
                    async move {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                        version
                    },
                    Message::DebouncedSearch,
                );
                return (pause, None);
            }
            Message::Search => return (self.start_search(auth), None),
            Message::DebouncedSearch(version) => {
                if self.is_current_search(version) {
                    return (self.start_search(auth), None);
                }
            }
            Message::SearchResult(version, result) => self.apply_search_result(version, result),
            Message::SelectRepo(index) => {
                if index < self.results.len() {
                    self.selected = Some(index);
                    self.refresh_destination();
                }
            }
            Message::UrlChanged(url) => {
                self.url = url;
                self.refresh_destination();
            }
            Message::DestinationChanged(destination) => self.destination = destination,
            Message::ChooseDestination => {
                let start = self.base_destination.display().to_string();
                return (
                    pick_folder(Some(start)).map(Message::DestinationChosen),
                    None,
                );
            }
            Message::DestinationChosen(Some(folder)) => {
                self.base_destination = PathBuf::from(folder);
                self.refresh_destination();
            }
            Message::DestinationChosen(None) | Message::FolderChosen(None) => {}
            Message::StartClone => {
                let Some(url) = self.clone_url() else {
                    return (Task::none(), None);
                };
                // The token is resolved inside the clone task.
                let request = CloneRequest {
                    url: url.trim().to_string(),
                    destination: PathBuf::from(self.destination.trim()),
                    token: None,
                    network: self.network.clone(),
                };
                return (self.begin_clone(request, auth), None);
            }
            Message::CloneProgress(progress) => {
                if let Some(started) = self.started {
                    let seconds = started.elapsed().as_secs_f64();
                    if seconds > 0.0 {
                        self.speed = progress.received_bytes as f64 / seconds;
                    }
                }
                self.progress = Some(progress);
            }
            Message::Frame => self.ease_progress(),
            Message::CloneResult(result) => {
                self.cloning = false;
                self.progress = None;
                let destination = self.active_destination.take();
                if result.is_ok() {
                    return (Task::none(), destination);
                }
            }
            Message::RetryClone => {
                if let (false, Some(request)) = (self.cloning, self.last_request.clone()) {
                    return (self.begin_clone(request, auth), None);
                }
            }
            Message::ChooseFolder => {
                return (pick_folder(None).map(Message::FolderChosen), None);
            }
            Message::FolderChosen(Some(folder)) => {
                self.checking_folder = true;
                self.folder_error = None;
                let path = PathBuf::from(folder);
                let check = Task::perform(
                    async move {
                        let candidate = path.clone();
                        let is_repo =
                            tokio::task::spawn_blocking(move || is_repository(&candidate))
                                .await
                                .unwrap_or(false);
                        (path, is_repo)
                    },
                    |(path, is_repo)| Message::FolderChecked(path, is_repo),
                );
                return (check, None);
            }
            Message::FolderChecked(path, is_repo) => {
                return (Task::none(), self.apply_folder_check(path, is_repo));
            }
            // Handled by `app.rs` before it reaches the panel.
            Message::OpenAccounts => {}
        }
        (Task::none(), None)
    }

    /// Moves the bar a fifth of the way to the clone's progress; snaps when close, or back
    /// when a new phase restarts at 0.
    fn ease_progress(&mut self) {
        let target = progress_ratio(self.progress.as_ref());
        let gap = target - self.shown_ratio;
        self.shown_ratio = if gap < 0.002 {
            target
        } else {
            self.shown_ratio + gap * 0.2
        };
    }

    /// Returns the folder to open when it holds a repository; otherwise says why not.
    fn apply_folder_check(&mut self, path: PathBuf, is_repo: bool) -> Option<PathBuf> {
        self.checking_folder = false;
        if is_repo {
            return Some(path);
        }
        self.folder_error = Some(tf(
            "add_repo_not_a_repo",
            &[("path", &path.display().to_string())],
        ));
        None
    }

    /// The URL "Clone" uses: the selected result, or the typed URL.
    fn clone_url(&self) -> Option<&str> {
        let url = match self.source {
            Source::GitHub | Source::GitLab => &self.results.get(self.selected?)?.url,
            Source::Url => &self.url,
            Source::LocalFolder => return None,
        };
        (!url.trim().is_empty()).then_some(url.as_str())
    }

    /// Sets the destination to the base folder plus the name of the repository to clone.
    fn refresh_destination(&mut self) {
        let base = &self.base_destination;
        self.destination = match self.clone_url().and_then(repo_name_from_url) {
            Some(name) => base.join(name),
            None => base.clone(),
        }
        .display()
        .to_string();
    }

    /// Searches the current query under a new version. The token is read in the task: the
    /// keyring can block.
    fn start_search(&mut self, auth: &AuthManager) -> Task<Message> {
        let query = self.query.trim().to_string();
        let Some(host) = self.source.host() else {
            return Task::none();
        };
        if query.len() < 2 {
            return Task::none();
        }
        self.search_version += 1;
        let version = self.search_version;
        let source = self.source;
        let network = self.network.clone();
        let auth = auth.clone();
        self.search_status = Some(t("add_repo_searching").to_string());

        Task::perform(
            async move {
                let host_url = format!("https://{host}");
                let token = tokio::task::spawn_blocking(move || auth.resolve_for_url(&host_url))
                    .await
                    .ok()
                    .flatten();
                search_repositories(source, &query, token.as_deref(), network).await
            },
            move |result| Message::SearchResult(version, result),
        )
    }

    /// Applies a search result, unless it answers a replaced search.
    fn apply_search_result(&mut self, version: u32, result: Result<Vec<RemoteRepo>, AppError>) {
        if !self.is_current_search(version) {
            return;
        }
        match result {
            Ok(results) => {
                self.search_status = results
                    .is_empty()
                    .then(|| t("add_repo_no_results").to_string());
                self.selected = (!results.is_empty()).then_some(0);
                self.results = results;
                self.refresh_destination();
            }
            Err(err) => self.search_status = Some(err.user_message()),
        }
    }

    fn begin_clone(&mut self, request: CloneRequest, auth: &AuthManager) -> Task<Message> {
        self.last_request = Some(request.clone());
        self.active_destination = Some(request.destination.clone());
        self.progress = None;
        self.shown_ratio = 0.0;
        self.speed = 0.0;
        self.started = Some(Instant::now());
        self.cloning = true;
        Task::run(
            clone_with_progress(request, auth.clone()),
            std::convert::identity,
        )
    }

    // -----------------------------------------------------------------------
    // View
    // -----------------------------------------------------------------------

    /// The screen; `is_connected(host)` tells whether an account is signed in there.
    pub fn view(&self, is_connected: impl Fn(&str) -> bool) -> Element<'_, Message> {
        let typo = Typography::current();
        let labels: Vec<String> = Source::ALL.iter().map(|s| s.label().to_string()).collect();
        let selected = Source::ALL
            .iter()
            .position(|&source| source == self.source)
            .unwrap_or(0);
        let mut content = column![
            text(t("add_repo_title"))
                .size(typo.sz(19))
                .font(typo.bold)
                .color(Palette::TEXT_PRIMARY()),
            segmented(&labels, selected, |index| Message::SelectSource(
                Source::ALL[index]
            )),
        ]
        .spacing(14);

        content = match self.source {
            Source::GitHub | Source::GitLab => {
                let signed_in = self.source.host().is_some_and(is_connected);
                content.push(self.search_view(signed_in))
            }
            Source::Url => content.push(field(
                text_input(t("add_repo_url_placeholder"), &self.url).on_input(Message::UrlChanged),
                None,
            )),
            Source::LocalFolder => content.push(self.folder_view()),
        };
        if self.source != Source::LocalFolder {
            content = content.push(self.destination_row());
            if self.cloning {
                content = content.push(self.progress_view());
            }
        }
        container(content.padding([22, 28]))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// The search field, the signed-out line, then the results.
    fn search_view(&self, signed_in: bool) -> Element<'_, Message> {
        let placeholder = if self.source == Source::GitHub {
            t("add_repo_search_github")
        } else {
            t("add_repo_search_gitlab")
        };
        let mut body = column![field(
            text_input(placeholder, &self.query)
                .on_input(Message::QueryChanged)
                .on_submit(Message::Search),
            None,
        )]
        .spacing(10);
        if !signed_in {
            body = body.push(
                row![
                    dim_line(t("add_repo_signed_out").to_string()),
                    gs_button::ghost(t("add_repo_connect"), Some(Message::OpenAccounts)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        if let Some(status) = &self.search_status {
            body = body.push(dim_line(status.clone()));
        }
        if !self.results.is_empty() {
            body = body.push(self.results_view());
        }
        body.height(Length::Fill).into()
    }

    /// One row per result, separated by rules, in a bordered box.
    fn results_view(&self) -> Element<'_, Message> {
        let now = chrono::Utc::now().timestamp();
        let mut rows = Column::new();
        for (index, repo) in self.results.iter().enumerate() {
            if index > 0 {
                rows = rows.push(horizontal_divider());
            }
            rows = rows.push(self.result_row(index, repo, now));
        }
        container(scrollable(rows))
            .style(|_| container::Style {
                border: Border::default()
                    .rounded(10)
                    .width(1)
                    .color(Palette::DIVIDER()),
                ..Default::default()
            })
            .into()
    }

    /// Owner initials, "owner / name" over the description, visibility, stars and date.
    fn result_row<'a>(&self, index: usize, repo: &'a RemoteRepo, now: i64) -> Element<'a, Message> {
        let typo = Typography::current();
        let (owner, name) = repo.owner_and_name();
        let mut title = row![].spacing(0);
        if !owner.is_empty() {
            title = title.push(
                text(format!("{owner} / "))
                    .size(typo.sz(13))
                    .font(typo.regular)
                    .color(Palette::TEXT_DIMMER()),
            );
        }
        title = title.push(
            text(name.to_string())
                .size(typo.sz(13))
                .font(typo.regular)
                .color(Palette::TEXT_PRIMARY()),
        );
        let mut details = column![title].spacing(2).width(Length::Fill);
        if let Some(description) = repo.description.as_deref().filter(|d| !d.is_empty()) {
            details = details.push(
                container(
                    text(description.to_string())
                        .size(typo.sz(11))
                        .font(typo.regular)
                        .color(Palette::TEXT_DIMMER())
                        .wrapping(text::Wrapping::None),
                )
                .width(Length::Fill)
                .clip(true),
            );
        }
        let mut line = row![
            avatar(&initials(owner), avatar_accent(owner), 28.0),
            details
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        if let Some(private) = repo.private {
            let label = if private {
                t("add_repo_private")
            } else {
                t("add_repo_public")
            };
            line = line.push(chip(label, Tone::Neutral));
        }
        let mut meta = vec![format!("★ {}", repo.stars)];
        if let Some(updated) = repo.updated {
            meta.push(relative_time(updated, now));
        }
        line = line.push(dim_line(meta.join(" · ")));

        let is_selected = self.selected == Some(index);
        button(line)
            .padding([10, 14])
            .width(Length::Fill)
            .on_press(Message::SelectRepo(index))
            .style(move |_, status| {
                let lit = is_selected || matches!(status, button::Status::Hovered);
                button::Style {
                    background: lit.then(|| Palette::BG_CARD_HOVER().into()),
                    text_color: Palette::TEXT_PRIMARY(),
                    ..Default::default()
                }
            })
            .into()
    }

    /// The folder picker, with the reason a folder was refused.
    fn folder_view(&self) -> Element<'_, Message> {
        let mut body = column![
            dim_line(t("add_repo_folder_hint").to_string()),
            gs_button::primary(
                t("add_repo_choose_folder"),
                (!self.checking_folder).then_some(Message::ChooseFolder),
            ),
        ]
        .spacing(10);
        if let Some(error) = &self.folder_error {
            let typo = Typography::current();
            body = body.push(
                text(error.clone())
                    .size(typo.sz(12))
                    .font(typo.regular)
                    .color(Palette::ERROR()),
            );
        }
        body.into()
    }

    /// "In", the destination field, Browse and Clone.
    fn destination_row(&self) -> Element<'_, Message> {
        let can_clone =
            !self.cloning && self.clone_url().is_some() && !self.destination.trim().is_empty();
        container(
            row![
                dim_line(t("add_repo_in").to_string()),
                field(
                    text_input("", &self.destination).on_input(Message::DestinationChanged),
                    None,
                ),
                gs_button::secondary(
                    t("add_repo_browse"),
                    (!self.cloning).then_some(Message::ChooseDestination),
                ),
                gs_button::primary(
                    t("add_repo_clone"),
                    can_clone.then_some(Message::StartClone)
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding(12)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Palette::BG_SIDEBAR().into()),
            border: Border::default().rounded(10),
            ..Default::default()
        })
        .into()
    }

    /// The bar and "Receiving objects · a / b · size · speed", then the delta count.
    fn progress_view(&self) -> Element<'_, Message> {
        let label = match &self.progress {
            None => t("add_repo_starting").to_string(),
            Some(p) if is_resolving(p) => tf(
                "add_repo_resolving",
                &[
                    ("done", &p.indexed_deltas.to_string()),
                    ("total", &p.total_deltas.to_string()),
                ],
            ),
            Some(p) => tf(
                "add_repo_receiving",
                &[
                    ("done", &p.received_objects.to_string()),
                    ("total", &p.total_objects.to_string()),
                    ("size", &format_size(p.received_bytes as f64)),
                    (
                        "speed",
                        &tf("add_repo_per_second", &[("size", &format_size(self.speed))]),
                    ),
                ],
            ),
        };
        let ratio = if colony_ui::effects_enabled() {
            self.shown_ratio
        } else {
            progress_ratio(self.progress.as_ref())
        };
        let bar = progress_bar(0.0..=1.0, ratio)
            .girth(6)
            .style(|_| progress_bar::Style {
                background: Palette::BG_CARD_HOVER().into(),
                bar: Palette::ACCENT().into(),
                border: Border::default().rounded(3),
            });
        column![bar, dim_line(label)].spacing(6).into()
    }
}

/// Receiving is done and the deltas are being resolved.
fn is_resolving(p: &CloneProgress) -> bool {
    p.total_deltas > 0 && p.received_objects >= p.total_objects
}

/// How far the current clone phase is, from 0 to 1.
fn progress_ratio(progress: Option<&CloneProgress>) -> f32 {
    match progress {
        None => 0.0,
        Some(p) if is_resolving(p) => p.indexed_deltas as f32 / p.total_deltas as f32,
        Some(p) => p.received_objects as f32 / p.total_objects.max(1) as f32,
    }
}

fn dim_line<'a>(label: String) -> Element<'a, Message> {
    let typo = Typography::current();
    text(label)
        .size(typo.sz(12))
        .font(typo.regular)
        .color(Palette::TEXT_DIMMER())
        .into()
}

/// "812 KB", "18.4 MB", "1.2 GB", with the locale's decimal mark.
fn format_size(bytes: f64) -> String {
    const KB: f64 = 1024.0;
    let (value, key) = if bytes >= KB * KB * KB {
        (bytes / (KB * KB * KB), "add_repo_size_gb")
    } else if bytes >= KB * KB {
        (bytes / (KB * KB), "add_repo_size_mb")
    } else {
        (bytes / KB, "add_repo_size_kb")
    };
    let value = format!("{value:.1}").replace('.', t("add_repo_decimal_mark"));
    tf(key, &[("value", &value)])
}

/// Whether `path` holds a Git repository. Blocking: run it off the UI thread.
fn is_repository(path: &Path) -> bool {
    RepoContext::from_path(path).is_some()
}

/// Opens the native folder picker off the UI thread, in `start` when given.
fn pick_folder(start: Option<String>) -> Task<Option<String>> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                let mut dialog = rfd::FileDialog::new();
                if let Some(start) = start {
                    dialog = dialog.set_directory(start);
                }
                dialog.pick_folder().map(|p| p.display().to_string())
            })
            .await
            .ok()
            .flatten()
        },
        std::convert::identity,
    )
}

// ---------------------------------------------------------------------------
// Fonctions utilitaires (extraction du nom de dépôt depuis une URL)
// ---------------------------------------------------------------------------

/// Extrait le nom du dépôt depuis une URL (HTTPS ou SSH).
fn repo_name_from_url(repo_url: &str) -> Option<String> {
    let trimmed = repo_url.trim().trim_end_matches('/');
    if let Ok(url) = Url::parse(trimmed) {
        if let Some(segment) = url
            .path_segments()
            .and_then(|mut segments| segments.rfind(|s| !s.is_empty()))
        {
            let name = segment.trim_end_matches(".git");
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }

    let mut candidate = trimmed;
    if let Some(idx) = trimmed.rfind(':') {
        candidate = &trimmed[idx + 1..];
    }

    if let Some(idx) = candidate.rfind('/') {
        candidate = &candidate[idx + 1..];
    }

    let name = candidate.trim_end_matches(".git");
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

// ---------------------------------------------------------------------------
// Fonctions réseau — identiques à l'original
// ---------------------------------------------------------------------------

/// Lance la recherche sur le fournisseur approprié.
async fn search_repositories(
    source: Source,
    query: &str,
    token: Option<&str>,
    network: NetworkOptions,
) -> Result<Vec<RemoteRepo>, AppError> {
    match source {
        Source::GitLab => search_gitlab(query, token, &network).await,
        _ => search_github(query, token, &network).await,
    }
}

/// Construit un client HTTP avec les en-têtes nécessaires (auth, proxy, timeout).
/// Le token part en `Authorization: Bearer`, que GitHub et GitLab acceptent pour un token
/// OAuth comme pour un jeton d'accès personnel.
async fn client_with_headers(
    token: Option<&str>,
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
        let auth_value = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|err| AppError::Validation(err.to_string()))?;
        headers.insert(AUTHORIZATION, auth_value);
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

/// Vérifie que l'URL respecte la politique HTTPS configurée.
fn enforce_https_policy(url: &str, network: &NetworkOptions) -> Result<(), AppError> {
    if url.starts_with("https://") && !network.use_https {
        return Err(AppError::Validation(t("network_https_blocked").to_string()));
    }

    if url.starts_with("http://") && network.use_https {
        return Err(AppError::Validation(t("network_http_blocked").to_string()));
    }

    Ok(())
}

// --- Structures de désérialisation GitHub ---

#[derive(Debug, Deserialize)]
struct GithubRepoItem {
    full_name: String,
    html_url: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    stargazers_count: u64,
    #[serde(default)]
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubSearchResponse {
    items: Vec<GithubRepoItem>,
}

/// Recherche GitHub : d'abord les dépôts du compte connecté et de ses organisations qui
/// correspondent à la requête, puis la recherche publique.
async fn search_github(
    query: &str,
    token: Option<&str>,
    network: &NetworkOptions,
) -> Result<Vec<RemoteRepo>, AppError> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let client = client_with_headers(token, None, network).await?;
    let mut unique = HashSet::new();

    let mut parts = Vec::new();
    if token.is_some() {
        parts.push(
            fetch_github_repos(
                &client,
                "https://api.github.com/user/repos",
                &[
                    ("visibility", "all"),
                    ("affiliation", "owner,collaborator,organization_member"),
                ],
                network,
            )
            .await
            .map(|repos| {
                repos
                    .into_iter()
                    .filter(|repo| matches_query(repo, query))
                    .collect()
            }),
        );
    }
    parts.push(search_github_public(&client, query, network).await);
    // Une requête refusée (limite de débit, réseau) ne masque pas les résultats des autres.
    let repositories = merge_partial_results(parts)?;

    let results = repositories
        .into_iter()
        .filter(|repo| unique.insert(repo.full_name.clone()))
        .map(|item| RemoteRepo {
            name: item.full_name,
            url: item.html_url,
            description: item.description,
            private: Some(item.private),
            stars: item.stargazers_count,
            updated: item.updated_at.as_deref().and_then(timestamp),
        })
        .collect();

    Ok(results)
}

// --- Structure de désérialisation GitLab ---

#[derive(Debug, Deserialize)]
struct GitlabProject {
    name_with_namespace: String,
    http_url_to_repo: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    star_count: u64,
    #[serde(default)]
    last_activity_at: Option<String>,
    /// "public", "internal" or "private".
    #[serde(default)]
    visibility: Option<String>,
}

impl GitlabProject {
    fn into_repo(self) -> RemoteRepo {
        RemoteRepo {
            name: self.name_with_namespace,
            url: self.http_url_to_repo,
            description: self.description,
            // Without a token GitLab sends no visibility and lists only public projects.
            // Internal projects need a GitLab account, so they show as private.
            private: Some(self.visibility.is_some_and(|v| v != "public")),
            stars: self.star_count,
            updated: self.last_activity_at.as_deref().and_then(timestamp),
        }
    }
}

/// Recherche les projets GitLab correspondant à la requête.
async fn search_gitlab(
    query: &str,
    token: Option<&str>,
    network: &NetworkOptions,
) -> Result<Vec<RemoteRepo>, AppError> {
    let base_url = "https://gitlab.com/api/v4/projects";
    enforce_https_policy(base_url, network)?;
    let client = client_with_headers(token, Some(("Accept", "application/json")), network).await?;

    let mut all_projects = Vec::new();
    let per_page = 100;

    for page in 1..=10 {
        let response = client
            .get(base_url)
            .query(&[
                ("search", query),
                ("per_page", &per_page.to_string()),
                ("page", &page.to_string()),
            ])
            .send()
            .await
            .map_err(AppError::from)?;

        if !response.status().is_success() {
            if page == 1 {
                // error_for_status() renvoie Err quand le statut n'est pas un succès
                match response.error_for_status() {
                    Err(err) => return Err(AppError::from(err)),
                    Ok(_) => {
                        return Err(AppError::Network(
                            t("add_repo_gitlab_unexpected").to_string(),
                        ))
                    }
                }
            }
            break;
        }

        let projects: Vec<GitlabProject> = response.json().await.map_err(AppError::from)?;
        let count = projects.len();
        all_projects.extend(projects);

        if count < per_page {
            break;
        }
    }

    Ok(all_projects
        .into_iter()
        .map(GitlabProject::into_repo)
        .collect())
}

/// Vrai quand la requête apparaît dans "owner/name" ou la description, sans tenir compte de la
/// casse.
fn matches_query(repo: &GithubRepoItem, query: &str) -> bool {
    let query = query.to_lowercase();
    repo.full_name.to_lowercase().contains(&query)
        || repo
            .description
            .as_deref()
            .is_some_and(|description| description.to_lowercase().contains(&query))
}

/// Recherche publique GitHub, première page de 100 résultats.
async fn search_github_public(
    client: &Client,
    query: &str,
    network: &NetworkOptions,
) -> Result<Vec<GithubRepoItem>, AppError> {
    let url = "https://api.github.com/search/repositories";
    enforce_https_policy(url, network)?;
    let response: GithubSearchResponse = client
        .get(url)
        .query(&[("q", query), ("per_page", "100")])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(response.items)
}

/// Réunit les résultats de plusieurs requêtes : les réponses obtenues sont gardées, et la
/// première erreur n'est renvoyée que si aucune requête n'a abouti.
fn merge_partial_results<T>(
    parts: impl IntoIterator<Item = Result<Vec<T>, AppError>>,
) -> Result<Vec<T>, AppError> {
    let mut items = Vec::new();
    let mut first_error = None;
    let mut any_ok = false;
    for part in parts {
        match part {
            Ok(part) => {
                any_ok = true;
                items.extend(part);
            }
            Err(err) => {
                first_error.get_or_insert(err);
            }
        }
    }
    match first_error {
        Some(err) if !any_ok => Err(err),
        _ => Ok(items),
    }
}

/// Seconds since the epoch of an RFC 3339 date, as both APIs send them.
fn timestamp(date: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(date)
        .ok()
        .map(|date| date.timestamp())
}

/// Récupère les dépôts GitHub depuis une URL paginée.
async fn fetch_github_repos(
    client: &Client,
    base_url: &str,
    params: &[(&str, &str)],
    network: &NetworkOptions,
) -> Result<Vec<GithubRepoItem>, AppError> {
    enforce_https_policy(base_url, network)?;
    let mut all_repos = Vec::new();
    let per_page = 100;

    for page in 1..=10 {
        let mut query_params: Vec<(&str, String)> =
            params.iter().map(|(k, v)| (*k, (*v).to_string())).collect();
        query_params.push(("per_page", per_page.to_string()));
        query_params.push(("page", page.to_string()));

        let response = client
            .get(base_url)
            .query(&query_params)
            .send()
            .await
            .map_err(AppError::from)?;

        if response.status() == StatusCode::NOT_FOUND {
            break;
        }

        let response = response.error_for_status().map_err(AppError::from)?;
        let mut repos: Vec<GithubRepoItem> = response.json().await.map_err(AppError::from)?;
        let count = repos.len();
        all_repos.append(&mut repos);

        if count < per_page {
            break;
        }
    }

    Ok(all_repos)
}

/// Clone bloquant lancé dans un thread dédié, qui envoie sa progression puis son résultat.
/// Le token de l'URL est résolu dans ce thread quand la requête n'en porte pas.
fn clone_with_progress(
    mut request: CloneRequest,
    auth: AuthManager,
) -> impl Stream<Item = Message> {
    iced::stream::channel(16, async move |mut output| {
        let mut progress = output.clone();
        let result = tokio::task::spawn_blocking(move || {
            if request.token.is_none() {
                request.token = auth.resolve_for_url(&request.url);
            }
            // shortcut: an update is dropped when the UI is 16 behind; the next one replaces it.
            clone_repository(request, move |update| {
                let _ = progress.try_send(Message::CloneProgress(update));
            })
        })
        .await
        .unwrap_or_else(|err| {
            Err(AppError::Network(tf(
                "add_repo_clone_interrupted",
                &[("error", &err.to_string())],
            )))
        });
        let _ = output.send(Message::CloneResult(result)).await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(name: &str) -> RemoteRepo {
        RemoteRepo {
            name: name.to_string(),
            url: format!("https://example.com/{name}.git"),
            description: None,
            private: None,
            stars: 0,
            updated: None,
        }
    }

    fn panel(base: &str) -> AddRepoPanel {
        AddRepoPanel::new(base.to_string(), NetworkOptions::default())
    }

    #[test]
    fn a_result_for_a_replaced_search_is_dropped() {
        let mut panel = panel("");
        panel.search_version = 3;

        panel.apply_search_result(2, Ok(vec![repo("old")]));
        assert!(panel.results.is_empty());
        assert_eq!(panel.search_status, None);

        panel.apply_search_result(3, Ok(vec![repo("new")]));
        assert_eq!(panel.results, vec![repo("new")]);
        assert_eq!(panel.selected, Some(0));
    }

    #[test]
    fn gitlab_rows_carry_the_project_visibility() {
        let projects: Vec<GitlabProject> = serde_json::from_str(
            r#"[
                {"name_with_namespace": "a / pub", "http_url_to_repo": "u", "visibility": "public"},
                {"name_with_namespace": "a / int", "http_url_to_repo": "u", "visibility": "internal"},
                {"name_with_namespace": "a / priv", "http_url_to_repo": "u", "visibility": "private"},
                {"name_with_namespace": "a / anon", "http_url_to_repo": "u"}
            ]"#,
        )
        .unwrap();
        let private: Vec<_> = projects
            .into_iter()
            .map(|p| p.into_repo().private)
            .collect();
        assert_eq!(
            private,
            vec![Some(false), Some(true), Some(true), Some(false)]
        );
    }

    #[test]
    fn the_progress_bar_eases_toward_the_clone_progress() {
        let mut panel = panel("");
        panel.cloning = true;
        panel.progress = Some(CloneProgress {
            received_objects: 10,
            total_objects: 10,
            ..CloneProgress::default()
        });

        panel.ease_progress();
        assert!(panel.shown_ratio > 0.0 && panel.shown_ratio < 1.0);

        for _ in 0..100 {
            panel.ease_progress();
        }
        assert_eq!(panel.shown_ratio, 1.0);

        // Resolving deltas starts again from 0: the bar jumps back instead of sliding down.
        panel.progress = Some(CloneProgress {
            received_objects: 10,
            total_objects: 10,
            total_deltas: 4,
            indexed_deltas: 0,
            received_bytes: 0,
        });
        panel.ease_progress();
        assert_eq!(panel.shown_ratio, 0.0);
    }

    #[test]
    fn partial_results_survive_a_failed_request() {
        let merged = merge_partial_results([
            Ok(vec![1, 2]),
            Err(AppError::Network("rate limited".into())),
            Ok(vec![3]),
        ]);
        assert_eq!(merged.unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn the_first_error_is_returned_when_every_request_fails() {
        let merged: Result<Vec<i32>, _> = merge_partial_results([
            Err(AppError::Network("first".into())),
            Err(AppError::Network("second".into())),
        ]);
        assert!(matches!(merged, Err(AppError::Network(msg)) if msg == "first"));
    }

    #[test]
    fn own_repositories_are_filtered_on_name_and_description() {
        let item = |full_name: &str, description: Option<&str>| GithubRepoItem {
            full_name: full_name.to_string(),
            html_url: String::new(),
            description: description.map(str::to_string),
            private: false,
            stargazers_count: 0,
            updated_at: None,
        };
        assert!(matches_query(
            &item("Project-Colony/Colony-Camera", None),
            "colony"
        ));
        assert!(matches_query(
            &item("MotherSphere/Raven", Some("A COLONY tool")),
            "colony"
        ));
        assert!(!matches_query(
            &item("MotherSphere/Raven", Some("Reverse")),
            "colony"
        ));
    }

    #[test]
    fn destination_appends_the_repo_name() {
        let base = Path::new("/home/me/Repositories");
        let mut panel = panel(&base.display().to_string());
        assert_eq!(panel.destination, base.display().to_string());

        // A search result picked on GitHub.
        panel.apply_search_result(panel.search_version, Ok(vec![repo("owner/Colony")]));
        assert_eq!(panel.destination, base.join("Colony").display().to_string());

        // A pasted SSH URL.
        panel.source = Source::Url;
        panel.url = "git@gitlab.com:group/GitSpace.git".into();
        panel.refresh_destination();
        assert_eq!(
            panel.destination,
            base.join("GitSpace").display().to_string()
        );

        // A new default folder from the preferences keeps the name appended.
        panel.set_default_destination("/srv/git".into());
        assert_eq!(
            panel.destination,
            Path::new("/srv/git").join("GitSpace").display().to_string()
        );
    }

    #[test]
    fn local_folder_that_is_not_a_repo_is_refused() {
        let plain = tempfile::tempdir().unwrap();
        let repository = tempfile::tempdir().unwrap();
        git2::Repository::init(repository.path()).unwrap();
        assert!(!is_repository(plain.path()));
        assert!(is_repository(repository.path()));

        let mut panel = panel("");
        panel.source = Source::LocalFolder;
        let opened = panel.apply_folder_check(plain.path().to_path_buf(), false);
        assert_eq!(opened, None);
        assert!(panel.folder_error.is_some());

        let opened = panel.apply_folder_check(repository.path().to_path_buf(), true);
        assert_eq!(opened.as_deref(), Some(repository.path()));
    }
}
