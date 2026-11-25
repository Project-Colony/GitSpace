//! The GitSpace application on iced 0.14: the model, the messages, `update` and `view`,
//! and the subscriptions (auto-fetch, toasts, keyboard, window focus).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use colony_ui::theme::Palette;
use iced::keyboard::{key::Named, Key, Modifiers};
use iced::widget::{column, container, row};
use iced::{Element, Length, Subscription, Task};

use crate::auth::AuthManager;
use crate::config::{AppConfig, Preferences};
use crate::error::logs_directory;
use crate::git::discover::discover_repositories;
use crate::git::status::{quick_status, QuickStatus};
use crate::i18n::{t, tf};
use crate::ui::{
    add_repo::{self, AddRepoPanel},
    branches::{self, BranchPanel},
    changes::{self, ChangesPanel},
    context::RepoContext,
    history::{self, HistoryPanel},
    preferences::{self, PreferencesPage},
    repo_header::{self, RepoHeader, RepoTab, TabCounts},
    sidebar::{self, Sidebar},
    stashes::{self, StashesPanel},
    widgets::toast::{Toast, ToastAction, Toasts},
};

/// Quick-status scans run at the same time, at most.
const STATUS_SCANS: usize = 4;

/// Status refreshes share one scan limit; a result older than the last one applied is dropped.
struct StatusRefresh {
    limit: Arc<tokio::sync::Semaphore>,
    started: u64,
    applied: u64,
}

impl Default for StatusRefresh {
    fn default() -> Self {
        Self {
            limit: Arc::new(tokio::sync::Semaphore::new(STATUS_SCANS)),
            started: 0,
            applied: 0,
        }
    }
}

impl StatusRefresh {
    /// Numbers a new refresh and hands it the shared limit.
    fn start(&mut self) -> (u64, Arc<tokio::sync::Semaphore>) {
        self.started += 1;
        (self.started, self.limit.clone())
    }

    /// Whether the result of refresh `generation` is newer than the one on screen.
    fn accept(&mut self, generation: u64) -> bool {
        if generation <= self.applied {
            return false;
        }
        self.applied = generation;
        true
    }
}

/// The panel that changed the repository, which [`GitSpaceApp::reload_other_panels`] skips.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Header,
    Changes,
    Branches,
    History,
    Stashes,
}

// ─── Message principal ─────────────────────────────────────────────────────

/// Message racine de l'application, enveloppe tous les sous-messages.
#[derive(Debug, Clone)]
pub enum Message {
    Sidebar(sidebar::Message),

    // Panneaux
    AddRepo(add_repo::Message),
    RepoHeader(repo_header::Message),
    Changes(changes::Message),
    History(crate::ui::history::Message),
    Branches(crate::ui::branches::Message),
    Stashes(stashes::Message),
    Preferences(preferences::Message),

    // Toasts
    ToastDismissed(u64),
    ToastAction(u64, ToastAction),

    // Chargement async du contexte d'un depot
    RepoContextLoaded(String, Option<RepoContext>),

    /// Removes expired toasts and redraws fading ones.
    ToastTick,

    // Auto-fetch timer tick
    AutoFetchTick,

    /// Ctrl+N (Cmd+N on macOS): show repository tab N, counted from 0.
    TabShortcut(usize),

    /// Closes the search, a menu or Preferences, the first one open.
    Escape,
    /// F2: rename the selected branch while Branches is shown.
    RenameBranch,

    /// The window gained focus: the repositories may have changed outside GitSpace.
    WindowFocused,

    /// Repositories found in the default clone folder.
    CloneFolderScanned(Vec<String>),
}

// ─── Modele principal ──────────────────────────────────────────────────────

/// Etat global de l'application GitSpace.
pub struct GitSpaceApp {
    active_tab: RepoTab,
    /// The Add repository screen is shown in place of the open repository.
    adding: bool,

    sidebar: Sidebar,
    /// Quick status of each listed repository, by path, from the last background scan.
    statuses: HashMap<String, QuickStatus>,
    status_refresh: StatusRefresh,

    // Panneaux
    add_repo: AddRepoPanel,
    repo_header: RepoHeader,
    changes_panel: ChangesPanel,
    history_panel: HistoryPanel,
    branches_panel: BranchPanel,
    stashes_panel: StashesPanel,
    preferences: PreferencesPage,

    // Etat global
    config: AppConfig,
    current_repo: Option<RepoContext>,
    /// Dernier depot demande : seul son resultat de chargement est applique.
    pending_repo: Option<String>,
    auth_manager: AuthManager,
    toasts: Toasts,
}

impl GitSpaceApp {
    /// Construit l'etat initial et retourne les taches de demarrage.
    ///
    /// `migration_error` is set when the files of an older build could not be moved.
    pub fn new(migration_error: Option<String>) -> (Self, Task<Message>) {
        let config = AppConfig::load();
        let preferences = config.preferences().clone();
        let default_clone_path = preferences.default_clone_path().to_string();
        preferences.standard.apply();
        colony_ui::i18n::set_locale(preferences.language().locale());
        let auth_manager =
            AuthManager::with_encrypted_fallback(preferences.allow_encrypted_tokens());
        let last_repo = startup_target(
            preferences.reopen_last_repo(),
            config.last_repo().map(|entry| entry.path.as_str()),
            |path| !std::path::Path::new(path).exists(),
        );

        let mut app = Self {
            sidebar: Sidebar::default(),
            statuses: HashMap::new(),
            status_refresh: StatusRefresh::default(),
            add_repo: AddRepoPanel::new(default_clone_path, preferences.network().clone()),
            repo_header: RepoHeader::new(preferences.network().clone()),
            history_panel: HistoryPanel::default(),
            branches_panel: BranchPanel::new(preferences.pinned_branches().to_vec()),
            changes_panel: ChangesPanel::default(),
            stashes_panel: StashesPanel::default(),
            preferences: PreferencesPage::new(auth_manager.clone()),
            config,
            current_repo: None,
            pending_repo: None,
            auth_manager,
            active_tab: RepoTab::Changes,
            adding: false,
            toasts: Toasts::default(),
        };

        if let Some(error) = migration_error {
            app.toasts.push(Toast::error(
                t("startup_migration_failed"),
                tf("startup_migration_failed_detail", &[("error", &error)]),
            ));
        }

        let accounts = app
            .preferences
            .accounts
            .refresh_accounts()
            .map(accounts_message);
        let repo = match last_repo {
            Some(path) => app.load_repo_context_async(path),
            None => Task::none(),
        };
        let scans = Task::batch([app.refresh_statuses(), app.rescan_clone_folder()]);
        (app, Task::batch([accounts, repo, scans]))
    }

    // ─── Update ────────────────────────────────────────────────────────

    /// Dispatch principal des messages.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Sidebar(msg) => self.update_sidebar(msg),

            // ── Add repository ─────────────────────────────────────────
            Message::AddRepo(add_repo::Message::OpenAccounts) => {
                self.preferences.open_accounts();
                Task::none()
            }
            Message::AddRepo(msg) => {
                self.notify_clone_failure(&msg);
                let (task, added) = self.add_repo.update(msg, &self.auth_manager);
                let load_task = match added {
                    Some(path) => {
                        // A cloned or opened repository shows on Changes.
                        self.active_tab = RepoTab::Changes;
                        self.load_repo_context_async(path.to_string_lossy().into_owned())
                    }
                    None => Task::none(),
                };
                Task::batch([task.map(Message::AddRepo), load_task])
            }

            // ── Repository header ──────────────────────────────────────
            Message::RepoHeader(repo_header::Message::SelectTab(tab)) => {
                self.active_tab = tab;
                Task::none()
            }
            Message::RepoHeader(msg) => {
                let finished = matches!(msg, repo_header::Message::ActionFinished { .. });
                let changed = remote_action_succeeded(
                    &msg,
                    self.current_repo.as_ref().map(RepoContext::path),
                );
                let (task, toast) = self.repo_header.update(msg, &self.auth_manager);
                if let Some(toast) = toast {
                    self.toasts.push(toast);
                }
                let reload = if changed {
                    self.reload_other_panels(Origin::Header)
                } else {
                    Task::none()
                };
                let statuses = if finished {
                    self.refresh_statuses()
                } else {
                    Task::none()
                };
                Task::batch([task.map(Message::RepoHeader), reload, statuses])
            }

            // ── Changes ────────────────────────────────────────────────
            Message::Changes(msg) => {
                // A commit moves HEAD: the other panels show the old state until reloaded.
                let changed = matches!(&msg, changes::Message::Committed(_, Ok(_)));
                let acted = changed || matches!(&msg, changes::Message::ActionDone(_, _, Ok(_)));
                let (task, toast) = self.changes_panel.update(msg);
                if let Some(toast) = toast {
                    self.toasts.push(toast);
                }
                let task = task.map(Message::Changes);
                let reload = if changed {
                    self.reload_other_panels(Origin::Changes)
                } else {
                    Task::none()
                };
                // A discard or stage also changes what a new stash would take.
                let statuses = if acted {
                    Task::batch([self.refresh_statuses(), self.reload_stashes()])
                } else {
                    Task::none()
                };
                Task::batch([task, reload, statuses])
            }

            // ── History ────────────────────────────────────────────────
            Message::History(msg) => {
                // A checkout moves HEAD and a new branch adds a ref: every panel reloads.
                let changed = matches!(&msg, history::Message::ActionDone(_, _, Ok(_)));
                let (task, toast) = self.history_panel.update(msg);
                if let Some(toast) = toast {
                    self.toasts.push(toast);
                }
                let mut tasks = vec![task.map(Message::History)];
                if changed {
                    tasks.push(self.reload_other_panels(Origin::History));
                    tasks.push(self.refresh_statuses());
                }
                Task::batch(tasks)
            }

            // ── Branches ───────────────────────────────────────────────
            Message::Branches(msg) => {
                // Checkout, merge and rebase change the working tree and HEAD.
                // A conflicted merge or rebase ends as Ok too: the tree still changed.
                let changed = matches!(&msg, branches::Message::ActionDone(_, _, Ok(_)));
                let (task, toast) = self.branches_panel.update(msg);
                if let Some(toast) = toast {
                    self.toasts.push(toast);
                }
                let mut tasks = vec![task.map(Message::Branches)];
                if changed {
                    tasks.push(self.reload_other_panels(Origin::Branches));
                    tasks.push(self.refresh_statuses());
                }

                // Gerer la demande de navigation vers l'historique d'une branche
                if let Some(branch) = self.branches_panel.take_history_request() {
                    self.active_tab = RepoTab::History;
                    if let Some(repo) = &self.current_repo {
                        tasks.push(
                            self.history_panel
                                .show_branch(branch, repo)
                                .map(Message::History),
                        );
                    }
                }

                // Persister les branches epinglees
                if let Some(pinned) = self.branches_panel.take_pinned_changes() {
                    let mut prefs = self.config.preferences().clone();
                    prefs.set_pinned_branches(pinned);
                    self.config.set_preferences(prefs);
                    self.save_config();
                }

                Task::batch(tasks)
            }

            // ── Stashes ────────────────────────────────────────────────
            Message::Stashes(msg) => {
                let changed = stash_job_finished(&msg);
                let (task, toast) = self.stashes_panel.update(msg);
                if let Some(toast) = toast {
                    self.toasts.push(toast);
                }
                let mut tasks = vec![task.map(Message::Stashes)];
                if changed {
                    tasks.push(self.reload_other_panels(Origin::Stashes));
                    tasks.push(self.refresh_statuses());
                }
                Task::batch(tasks)
            }

            // ── Preferences ────────────────────────────────────────────
            // Every change applies and saves at once: there is no Save button.
            Message::Preferences(msg) => {
                let mut prefs = self.config.preferences().clone();
                let mut logging = *self.config.logging();
                let (task, toast) = self.preferences.update(msg, &mut prefs, &mut logging);
                if let Some(toast) = toast {
                    self.toasts.push(toast);
                }
                let logging_changed = logging != *self.config.logging();
                self.config.set_logging(logging);
                let applied = if prefs != *self.config.preferences() {
                    self.apply_preferences(prefs)
                } else {
                    if logging_changed {
                        self.save_config();
                    }
                    Task::none()
                };
                Task::batch([task.map(Message::Preferences), applied])
            }

            // ── Toasts ─────────────────────────────────────────────────
            Message::ToastDismissed(id) => {
                self.toasts.dismiss(id);
                Task::none()
            }

            Message::ToastAction(id, action) => {
                self.toasts.dismiss(id);
                self.handle_toast_action(action)
            }

            Message::ToastTick => {
                self.toasts.expire(Instant::now());
                Task::none()
            }

            // ── Chargement async du contexte depot ──────────────────────
            Message::RepoContextLoaded(path, context) => {
                // Un chargement plus recent a ete demande : ce resultat est perime.
                if self.pending_repo.as_deref() != Some(path.as_str()) {
                    return Task::none();
                }
                self.pending_repo = None;
                let Some(repo) = context else {
                    // The repository open until now stays open.
                    self.toasts.push(Toast::error(
                        t("repo_missing_title"),
                        tf("repo_missing_detail", &[("path", &path)]),
                    ));
                    return self.refresh_statuses();
                };
                if self.config.open_repo(&path, chrono::Utc::now().timestamp()) {
                    self.save_config();
                }
                self.adding = false;
                let tasks = [
                    self.repo_header.load(&repo).map(Message::RepoHeader),
                    self.changes_panel.load(&repo).map(Message::Changes),
                    self.branches_panel.load(&repo).map(Message::Branches),
                    self.history_panel.load(&repo).map(Message::History),
                    self.stashes_panel.load(&repo).map(Message::Stashes),
                ];
                self.current_repo = Some(repo);
                Task::batch(tasks)
            }

            // ── Auto-fetch ─────────────────────────────────────────────
            Message::AutoFetchTick => Task::batch([
                self.maybe_start_auto_fetch(),
                self.refresh_statuses(),
                self.reload_changes(),
            ]),

            Message::TabShortcut(index) => {
                if let Some(tab) = RepoTab::ALL.get(index) {
                    self.active_tab = *tab;
                    self.adding = false;
                }
                Task::none()
            }

            Message::Escape => {
                if self.sidebar.is_searching() {
                    self.sidebar.close_search();
                } else if self.sidebar.is_menu_open() {
                    self.sidebar.set_menu(None);
                } else if self.preferences.is_open() {
                    return self.update(Message::Preferences(preferences::Message::Toggle));
                }
                Task::none()
            }

            Message::RenameBranch => {
                if self.active_tab == RepoTab::Branches && !self.adding {
                    return self.update(Message::Branches(branches::Message::StartRename));
                }
                Task::none()
            }

            // A commit, a checkout or a fetch made elsewhere changes what every panel shows;
            // History is left alone so that switching windows keeps its selection.
            Message::WindowFocused => {
                let panels = match self.current_repo.clone() {
                    Some(repo) => Task::batch([
                        self.repo_header.load(&repo).map(Message::RepoHeader),
                        self.branches_panel.load(&repo).map(Message::Branches),
                    ]),
                    None => Task::none(),
                };
                Task::batch([self.refresh_statuses(), self.reload_changes(), panels])
            }

            Message::CloneFolderScanned(paths) => {
                if !self.config.merge_discovered(&paths) {
                    return Task::none();
                }
                self.save_config();
                self.refresh_statuses()
            }
        }
    }

    fn update_sidebar(&mut self, message: sidebar::Message) -> Task<Message> {
        use sidebar::Message as Side;
        match message {
            Side::ToggleSearch => self
                .sidebar
                .toggle_search(self.config.repos())
                .map(Message::Sidebar),
            Side::Query(query) => {
                self.sidebar.set_query(query, self.config.repos());
                Task::none()
            }
            Side::MoveSelection(delta) => {
                let target = self.sidebar.step(delta, self.config.repos());
                match target {
                    // While searching, Up/Down only move the cursor; Enter opens.
                    Some(path) if !self.sidebar.is_searching() => self.open_repo(&path),
                    _ => Task::none(),
                }
            }
            Side::Activate => match self.sidebar.cursor() {
                Some(path) => self.open_repo(&path),
                None => Task::none(),
            },
            Side::Select(path) => self.open_repo(&path),
            Side::Pin(path, pinned) => {
                self.sidebar.set_menu(None);
                self.config.set_pinned(&path, pinned);
                self.save_config();
                Task::none()
            }
            Side::Hide(path) => {
                self.sidebar.set_menu(None);
                self.config.hide_repo(&path);
                self.save_config();
                Task::none()
            }
            Side::OpenFolder(path) => {
                self.sidebar.set_menu(None);
                if let Err(err) = repo_header::open_folder(&path) {
                    self.toasts.push(Toast::error(
                        t("open_folder_failed"),
                        format!("{path}: {err}"),
                    ));
                }
                Task::none()
            }
            Side::OpenTerminal(path) => {
                self.sidebar.set_menu(None);
                if let Err(err) = repo_header::open_terminal(&path) {
                    self.toasts.push(Toast::error(
                        t("open_terminal_failed"),
                        format!("{path}: {err}"),
                    ));
                }
                Task::none()
            }
            Side::ContextMenu(path) => {
                self.sidebar.set_menu(path);
                Task::none()
            }
            Side::AddRepository => {
                let closed = if self.preferences.is_open() {
                    self.update(Message::Preferences(preferences::Message::Toggle))
                } else {
                    Task::none()
                };
                self.adding = true;
                closed
            }
            Side::TogglePreferences => {
                self.update(Message::Preferences(preferences::Message::Toggle))
            }
            Side::StatusesLoaded(generation, statuses) => {
                if self.status_refresh.accept(generation) {
                    self.statuses.extend(statuses);
                }
                Task::none()
            }
        }
    }

    /// Opens `path` from the sidebar; the search and any menu close.
    fn open_repo(&mut self, path: &str) -> Task<Message> {
        self.adding = false;
        self.sidebar.close_search();
        self.sidebar.set_menu(None);
        let current = self.current_repo.as_ref().map(RepoContext::path);
        if !needs_load(&mut self.pending_repo, current, path) {
            self.sidebar.set_anchor(path);
            return Task::none();
        }
        self.load_repo_context_async(path.to_string())
    }

    /// Reads the quick status of every listed repository in the background, a few at a time.
    fn refresh_statuses(&mut self) -> Task<Message> {
        let paths: Vec<String> = self
            .config
            .repos()
            .iter()
            .filter(|entry| !entry.hidden)
            .map(|entry| entry.path.clone())
            .collect();
        if paths.is_empty() {
            return Task::none();
        }
        let (generation, limit) = self.status_refresh.start();
        Task::perform(
            async move {
                let mut scans = tokio::task::JoinSet::new();
                for path in paths {
                    let limit = limit.clone();
                    scans.spawn(async move {
                        let _permit = limit.acquire_owned().await.ok()?;
                        let status = {
                            let path = path.clone();
                            tokio::task::spawn_blocking(move || quick_status(&path))
                                .await
                                .ok()?
                        };
                        Some((path, status))
                    });
                }
                let mut statuses = HashMap::new();
                while let Some(scan) = scans.join_next().await {
                    if let Ok(Some((path, status))) = scan {
                        statuses.insert(path, status);
                    }
                }
                statuses
            },
            move |statuses| {
                Message::Sidebar(sidebar::Message::StatusesLoaded(generation, statuses))
            },
        )
    }

    /// Looks for repositories in the default clone folder, when that option is on.
    fn rescan_clone_folder(&self) -> Task<Message> {
        let preferences = self.config.preferences();
        if !preferences.scan_clone_folder() {
            return Task::none();
        }
        let root = PathBuf::from(preferences.default_clone_path());
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    discover_repositories(&root)
                        .into_iter()
                        .map(|path| path.to_string_lossy().into_owned())
                        .collect()
                })
                .await
                .unwrap_or_default()
            },
            Message::CloneFolderScanned,
        )
    }

    /// Rereads the open repository's changes and stashes, which other programs may have
    /// edited.
    fn reload_changes(&mut self) -> Task<Message> {
        let changes = match self.current_repo.clone() {
            Some(repo) => self.changes_panel.load(&repo).map(Message::Changes),
            None => Task::none(),
        };
        Task::batch([changes, self.reload_stashes()])
    }

    fn reload_stashes(&mut self) -> Task<Message> {
        match self.current_repo.clone() {
            Some(repo) => self.stashes_panel.load(&repo).map(Message::Stashes),
            None => Task::none(),
        }
    }

    /// Reloads every repository panel except `origin` after it changed the repository.
    fn reload_other_panels(&mut self, origin: Origin) -> Task<Message> {
        let Some(repo) = self.current_repo.clone() else {
            return Task::none();
        };
        let mut tasks = Vec::new();
        if origin != Origin::Header {
            tasks.push(self.repo_header.load(&repo).map(Message::RepoHeader));
        }
        if origin != Origin::Changes {
            tasks.push(self.changes_panel.load(&repo).map(Message::Changes));
        }
        if origin != Origin::Branches {
            tasks.push(self.branches_panel.load(&repo).map(Message::Branches));
        }
        if origin != Origin::Stashes {
            tasks.push(self.stashes_panel.load(&repo).map(Message::Stashes));
        }
        tasks.push(self.history_panel.load(&repo).map(Message::History));
        Task::batch(tasks)
    }

    // ─── View ──────────────────────────────────────────────────────────

    /// Compose la vue complete de l'application.
    pub fn view(&self) -> Element<'_, Message> {
        let sidebar = self
            .sidebar
            .view(
                self.config.repos(),
                &self.statuses,
                self.current_repo.as_ref().map(RepoContext::path),
                self.preferences.is_open(),
            )
            .map(Message::Sidebar);

        // Preferences take the place of the content area while open; the sidebar stays.
        let main_area: Element<'_, Message> = if self.preferences.is_open() {
            self.preferences.view(
                self.config.preferences(),
                self.config.logging(),
                Message::Preferences,
                Message::Sidebar(sidebar::Message::TogglePreferences),
            )
        } else {
            match (&self.current_repo, self.adding) {
                (Some(_), false) => {
                    let counts = TabCounts {
                        changes: self.changes_panel.count(),
                        stashes: self.stashes_panel.count(),
                    };
                    let header = self
                        .repo_header
                        .view(self.active_tab, counts)
                        .map(Message::RepoHeader);
                    column![header, self.view_active_tab()]
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                }
                (_, _) => self
                    .add_repo
                    .view(|host| self.preferences.accounts.is_connected(host))
                    .map(Message::AddRepo),
            }
        };

        let toast_overlay = self.toast_overlay();
        let page = row![sidebar, main_area].height(Length::Fill);

        // Utiliser un container pour le fond + overlay notifications
        let base = container(page)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_: &iced::Theme| container::Style {
                background: Some(Palette::BG_PRIMARY().into()),
                ..Default::default()
            });

        if self.toasts.is_empty() {
            base.into()
        } else {
            // Placer les notifications en overlay via une colonne empilee
            column![iced::widget::stack![
                base,
                container(toast_overlay)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .padding(iced::Padding {
                        top: 60.0,
                        right: 16.0,
                        bottom: 0.0,
                        left: 0.0
                    }),
            ]
            .width(Length::Fill)
            .height(Length::Fill),]
            .into()
        }
    }

    /// The panel of the selected repository tab.
    fn view_active_tab(&self) -> Element<'_, Message> {
        match self.active_tab {
            // The Changes columns run to the window edges, as in the mockup.
            RepoTab::Changes => self.changes_panel.view().map(Message::Changes),
            RepoTab::Stashes => self.stashes_panel.view().map(Message::Stashes),
            RepoTab::History => self.history_panel.view().map(Message::History),
            RepoTab::Branches => self.branches_panel.view().map(Message::Branches),
        }
    }

    fn toast_overlay(&self) -> Element<'_, Message> {
        self.toasts
            .view(Message::ToastDismissed, Message::ToastAction)
    }

    // ─── Theme ─────────────────────────────────────────────────────────

    /// The iced theme for the built-in widgets, from the active colony-ui palette.
    pub fn theme(&self) -> iced::Theme {
        let palette = iced::theme::Palette {
            background: Palette::BG_PRIMARY(),
            text: Palette::TEXT_PRIMARY(),
            primary: Palette::ACCENT(),
            success: Palette::SUCCESS(),
            warning: Palette::WARNING(),
            danger: Palette::ERROR(),
        };
        iced::Theme::custom_with_fn("GitSpace".to_string(), palette, |palette| {
            iced::theme::palette::Extended::generate(palette)
        })
    }

    // ─── Subscriptions ─────────────────────────────────────────────────

    /// Abonnements actifs : tick des toasts + timer auto-fetch.
    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = Vec::new();

        // Expire toasts once a second; every frame while one fades out.
        if self.toasts.is_fading(Instant::now()) {
            subs.push(iced::window::frames().map(|_| Message::ToastTick));
        } else if !self.toasts.is_empty() {
            subs.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::ToastTick));
        }

        if self.add_repo.is_easing() {
            subs.push(iced::window::frames().map(|_| Message::AddRepo(add_repo::Message::Frame)));
        }

        // Timer auto-fetch. Il est recree apres chaque operation du panneau, donc le prochain
        // auto-fetch part un intervalle apres la derniere operation reseau ou l'ouverture du depot.
        if self.config.preferences().auto_fetch_enabled()
            && self.current_repo.is_some()
            && !self.repo_header.is_busy()
        {
            let interval =
                Duration::from_secs(self.config.preferences().auto_fetch_interval_minutes() * 60);
            subs.push(iced::time::every(interval).map(|_| Message::AutoFetchTick));
        }

        // Every key press, with whether a widget took it: the search field takes Escape.
        subs.push(
            iced::event::listen_with(|event, status, _| match event {
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key, modifiers, ..
                }) => Some((key, modifiers, status == iced::event::Status::Captured)),
                _ => None,
            })
            .with(self.sidebar.is_searching())
            .filter_map(|(searching, (key, modifiers, captured))| {
                shortcut(key, modifiers, captured, searching)
            }),
        );

        subs.push(iced::window::events().filter_map(|(_, event)| {
            matches!(event, iced::window::Event::Focused).then_some(Message::WindowFocused)
        }));

        Subscription::batch(subs)
    }

    // ─── Helpers prives ────────────────────────────────────────────────

    /// Sauvegarde la configuration et notifie l'utilisateur en cas d'echec.
    fn save_config(&mut self) {
        if let Err(err) = self.config.save() {
            self.toasts
                .push(Toast::error(t("config_save_failed"), err.to_string()));
        }
    }

    /// Lance le chargement async du contexte d'un depot.
    /// Les I/O git sont deplacees dans un thread bloquant pour ne pas
    /// bloquer le thread UI.
    fn load_repo_context_async(&mut self, path: String) -> Task<Message> {
        self.sidebar.set_anchor(&path);
        self.pending_repo = Some(path.clone());
        Task::perform(
            async move {
                let p = path.clone();
                let context = tokio::task::spawn_blocking(move || RepoContext::from_path(&p))
                    .await
                    .ok()
                    .flatten();
                (path, context)
            },
            |(path, context)| Message::RepoContextLoaded(path, context),
        )
    }

    /// Applique un jeu de preferences mis a jour a toute l'application.
    fn apply_preferences(&mut self, preferences: Preferences) -> Task<Message> {
        let old = self.config.preferences();
        let rescan = old.scan_clone_folder() != preferences.scan_clone_folder()
            || old.default_clone_path() != preferences.default_clone_path();
        let storage_changed = old.allow_encrypted_tokens() != preferences.allow_encrypted_tokens();
        self.config.set_preferences(preferences.clone());
        // An import or a reset changes these too.
        preferences.standard.apply();
        colony_ui::i18n::set_locale(preferences.language().locale());

        // Propager aux panneaux
        self.add_repo
            .set_default_destination(preferences.default_clone_path().to_string());
        self.add_repo
            .set_network_preferences(preferences.network().clone());
        self.repo_header
            .set_network_preferences(preferences.network().clone());
        self.branches_panel
            .set_pinned_branches(preferences.pinned_branches().to_vec());
        let accounts = if storage_changed {
            self.auth_manager
                .set_encrypted_fallback(preferences.allow_encrypted_tokens());
            self.preferences
                .accounts
                .set_auth_manager(self.auth_manager.clone())
                .map(accounts_message)
        } else {
            Task::none()
        };

        self.save_config();
        let scan = if rescan {
            self.rescan_clone_folder()
        } else {
            Task::none()
        };
        Task::batch([accounts, scan])
    }

    fn handle_toast_action(&mut self, action: ToastAction) -> Task<Message> {
        match action {
            ToastAction::Retry => {
                let (task, _) = self
                    .add_repo
                    .update(add_repo::Message::RetryClone, &self.auth_manager);
                task.map(Message::AddRepo)
            }
            ToastAction::CopyLogPath(path) => iced::clipboard::write(path.display().to_string()),
        }
    }

    /// Signale un échec de clonage ou de recherche, avec le chemin des logs.
    fn notify_clone_failure(&mut self, message: &add_repo::Message) {
        let (title, err, can_retry) = match message {
            add_repo::Message::CloneResult(Err(err)) => (t("add_repo_clone_failed"), err, true),
            add_repo::Message::SearchResult(version, Err(err))
                if self.add_repo.is_current_search(*version) =>
            {
                (t("add_repo_search_failed"), err, false)
            }
            _ => return,
        };
        let body = format!("{}\n{}", err.user_message(), err.detail());
        let mut toast = Toast::error(title, body);
        if can_retry {
            toast = toast.with_action(ToastAction::Retry);
        }
        self.toasts
            .push(toast.with_action(ToastAction::CopyLogPath(logs_directory())));
    }

    /// Demarre l'auto-fetch du depot courant si l'option est active.
    fn maybe_start_auto_fetch(&mut self) -> Task<Message> {
        if !self.config.preferences().auto_fetch_enabled() {
            return Task::none();
        }
        let Some(repo) = self.current_repo.as_ref() else {
            return Task::none();
        };
        self.repo_header
            .auto_fetch(repo, &self.auth_manager)
            .map(Message::RepoHeader)
    }
}

/// The window-wide shortcuts. A key a widget already `captured` does nothing, except
/// Escape while searching: the focused search field takes it before anyone else.
fn shortcut(key: Key, modifiers: Modifiers, captured: bool, searching: bool) -> Option<Message> {
    if key == Key::Named(Named::Escape) {
        return (!captured || searching).then_some(Message::Escape);
    }
    if captured {
        return None;
    }
    if key == Key::Named(Named::F2) && modifiers.is_empty() {
        return Some(Message::RenameBranch);
    }
    let side = |message| Some(Message::Sidebar(message));
    match key.as_ref() {
        Key::Named(arrow @ (Named::ArrowUp | Named::ArrowDown))
            if modifiers == Modifiers::ALT || (searching && modifiers.is_empty()) =>
        {
            side(sidebar::Message::MoveSelection(
                if arrow == Named::ArrowUp { -1 } else { 1 },
            ))
        }
        Key::Character(c) if modifiers.command() && !modifiers.alt() && !modifiers.shift() => {
            match c {
                "k" => side(sidebar::Message::ToggleSearch),
                "," => side(sidebar::Message::TogglePreferences),
                _ => c
                    .parse::<usize>()
                    .ok()
                    .filter(|digit| (1..=RepoTab::ALL.len()).contains(digit))
                    .map(|digit| Message::TabShortcut(digit - 1)),
            }
        }
        _ => None,
    }
}

/// Whether a fetch, pull or push on the open repository `current` succeeded: the remote
/// branches, and after a pull HEAD and the working tree, then differ from what the panels show.
fn remote_action_succeeded(msg: &repo_header::Message, current: Option<&str>) -> bool {
    matches!(
        msg,
        repo_header::Message::ActionFinished { path, result: Ok(_), .. }
            if current == Some(&**path)
    )
}

fn accounts_message(message: preferences::accounts::Message) -> Message {
    Message::Preferences(preferences::Message::Accounts(message))
}

/// Returns whether `path` must be loaded. Picking the open repository again drops a slower
/// load requested before it, so that result cannot replace the repository picked last.
fn needs_load(pending: &mut Option<String>, current: Option<&str>, path: &str) -> bool {
    if current != Some(path) {
        return true;
    }
    *pending = None;
    false
}

/// The repository to open on Changes at startup; `None` shows Add repository.
fn startup_target(
    reopen_last_repo: bool,
    last_repo: Option<&str>,
    is_missing: impl Fn(&str) -> bool,
) -> Option<String> {
    last_repo
        .filter(|path| reopen_last_repo && !is_missing(path))
        .map(str::to_string)
}

/// Any finished stash job may have changed the working tree: an Apply and drop that
/// left conflicts, or applied and then failed to drop, still returns an error.
fn stash_job_finished(msg: &stashes::Message) -> bool {
    matches!(msg, stashes::Message::Done(..))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(message: Option<Message>) -> String {
        message.map_or_else(|| "None".to_string(), |message| format!("{message:?}"))
    }

    #[test]
    fn every_remote_action_that_succeeded_here_reloads_the_panels() {
        let finished = |path: &str, action, result| repo_header::Message::ActionFinished {
            path: Arc::from(path),
            action,
            result,
        };
        use repo_header::RemoteAction::{AutoFetch, Fetch, Pull, Push};
        for action in [Fetch, AutoFetch, Pull, Push] {
            let done = finished("/r", action, Ok("done".into()));
            assert!(remote_action_succeeded(&done, Some("/r")), "{action:?}");
            assert!(!remote_action_succeeded(&done, Some("/other")));
            assert!(!remote_action_succeeded(&done, None));
            let failed = finished("/r", action, Err("offline".into()));
            assert!(!remote_action_succeeded(&failed, Some("/r")));
        }
    }

    #[test]
    fn keyboard_map_covers_search_tabs_and_preferences() {
        let key = |c: &str| Key::Character(c.into());
        let up = Key::Named(Named::ArrowUp);
        let down = Key::Named(Named::ArrowDown);
        let escape = Key::Named(Named::Escape);
        let cmd = Modifiers::COMMAND;
        let free = |key, modifiers| name(shortcut(key, modifiers, false, false));

        assert_eq!(free(key("k"), cmd), "Sidebar(ToggleSearch)");
        assert_eq!(free(key(","), cmd), "Sidebar(TogglePreferences)");
        assert_eq!(free(key("1"), cmd), "TabShortcut(0)");
        assert_eq!(free(key("4"), cmd), "TabShortcut(3)");
        assert_eq!(free(key("5"), cmd), "None");
        let f2 = Key::Named(Named::F2);
        assert_eq!(free(f2.clone(), Modifiers::empty()), "RenameBranch");
        // F2 inside a text field stays with the field.
        assert_eq!(name(shortcut(f2, Modifiers::empty(), true, false)), "None");
        assert_eq!(free(key("3"), cmd | Modifiers::SHIFT), "None");
        assert_eq!(free(key("k"), Modifiers::empty()), "None");
        assert_eq!(
            free(up.clone(), Modifiers::ALT),
            "Sidebar(MoveSelection(-1))"
        );
        assert_eq!(
            free(down.clone(), Modifiers::ALT),
            "Sidebar(MoveSelection(1))"
        );
        // Plain arrows only move through the list while the search is open.
        assert_eq!(free(down.clone(), Modifiers::empty()), "None");
        assert_eq!(
            name(shortcut(down, Modifiers::empty(), false, true)),
            "Sidebar(MoveSelection(1))"
        );
        assert_eq!(free(escape.clone(), Modifiers::empty()), "Escape");

        // Keys typed into a field reach nothing, but the search field's Escape closes it.
        assert_eq!(name(shortcut(key("k"), cmd, true, false)), "None");
        assert_eq!(
            name(shortcut(escape.clone(), Modifiers::empty(), true, false)),
            "None"
        );
        assert_eq!(
            name(shortcut(escape, Modifiers::empty(), true, true)),
            "Escape"
        );
    }

    #[test]
    fn startup_target_prefers_the_last_repo() {
        let none_missing = |_: &str| false;
        let gone = |path: &str| path == "/gone";
        assert_eq!(
            startup_target(true, Some("/repo"), none_missing),
            Some("/repo".to_string())
        );
        // Turned off, nothing opened yet or the folder is missing: Add repository.
        assert_eq!(startup_target(false, Some("/repo"), none_missing), None);
        assert_eq!(startup_target(true, None, none_missing), None);
        assert_eq!(startup_target(true, Some("/gone"), gone), None);
    }

    #[test]
    fn picking_the_open_repository_drops_a_slower_pending_load() {
        // /a is open and a load of /b is still running.
        let mut pending = Some("/b".to_string());
        assert!(!needs_load(&mut pending, Some("/a"), "/a"));
        // RepoContextLoaded(/b) now fails the pending check and leaves /a open.
        assert_eq!(pending, None);

        assert!(needs_load(&mut pending, Some("/a"), "/b"));
        assert!(needs_load(&mut pending, None, "/a"));
    }

    #[test]
    fn a_failed_stash_job_still_reloads_the_other_panels() {
        let done =
            |result| stashes::Message::Done(Arc::from("/r"), "stashes_apply_drop_failed", result);
        assert!(stash_job_finished(&done(Ok("applied".into()))));
        // Apply and drop that kept conflicts: the tree changed, the result is Err.
        assert!(stash_job_finished(&done(Err(
            t("stashes_conflicts_kept").to_string()
        ))));
        assert!(!stash_job_finished(&stashes::Message::SelectFile(
            "a.txt".into()
        )));
    }

    #[test]
    fn status_refreshes_share_one_limit_and_drop_older_results() {
        let mut refresh = StatusRefresh::default();
        let (first, first_limit) = refresh.start();
        let (second, second_limit) = refresh.start();
        assert!(Arc::ptr_eq(&first_limit, &second_limit));
        assert_eq!(first_limit.available_permits(), STATUS_SCANS);

        // The refresh started after a commit lands first; the older one must not undo it.
        assert!(refresh.accept(second));
        assert!(!refresh.accept(first));
        let (third, _) = refresh.start();
        assert!(refresh.accept(third));
    }
}
