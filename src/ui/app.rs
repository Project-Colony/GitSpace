//! Application principale GitSpace — architecture Elm pour Iced 0.13.
//!
//! Centralise le modèle, les messages, le dispatch update/view,
//! les subscriptions (auto-fetch, notifications, clavier) et le thème.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::keyboard::{key::Named, Key, Modifiers};
use iced::widget::{column, container, row};
use iced::{Element, Length, Subscription, Task};

use crate::auth::AuthManager;
use crate::config::{AppConfig, LoggingOptions, Preferences};
use crate::error::logs_directory;
use crate::git::discover::discover_repositories;
use crate::git::status::{quick_status, QuickStatus};
use crate::i18n::{t, tf};
use crate::ui::{
    auth::AuthPanel,
    branches::{self, BranchPanel},
    clone::{self, ClonePanel},
    context::RepoContext,
    history::HistoryPanel,
    layout::{self, LayoutMessage, MainTab},
    preferences::PreferencesPanel,
    recent::RecentList,
    repo_overview::{self, RepoOverviewPanel},
    sidebar::{self, Sidebar},
    stage::{self, StagePanel},
    theme::Theme,
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

/// What Ctrl+1 to Ctrl+4 show. Stashes stay on the Stage panel until they get their own tab.
const REPO_TABS: [MainTab; 4] = [
    MainTab::Stage,
    MainTab::History,
    MainTab::Branches,
    MainTab::Stage,
];

// ─── Message principal ─────────────────────────────────────────────────────

/// Message racine de l'application, enveloppe tous les sous-messages.
#[derive(Debug, Clone)]
pub enum Message {
    // Layout / navigation
    Layout(LayoutMessage),

    Sidebar(sidebar::Message),

    // Panneaux
    Clone(crate::ui::clone::Message),
    Recent(crate::ui::recent::Message),
    RepoOverview(crate::ui::repo_overview::Message),
    Stage(crate::ui::stage::Message),
    History(crate::ui::history::Message),
    Branches(crate::ui::branches::Message),
    Auth(crate::ui::auth::Message),
    Preferences(crate::ui::preferences::Message),

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

    /// The window gained focus: the repositories may have changed outside GitSpace.
    WindowFocused,

    /// Repositories found in the default clone folder.
    CloneFolderScanned(Vec<String>),
}

// ─── Modele principal ──────────────────────────────────────────────────────

/// Etat global de l'application GitSpace.
pub struct GitSpaceApp {
    theme: Theme,
    active_tab: MainTab,
    tab_order: Vec<MainTab>,

    sidebar: Sidebar,
    /// Quick status of each listed repository, by path, from the last background scan.
    statuses: HashMap<String, QuickStatus>,
    status_refresh: StatusRefresh,

    // Panneaux
    clone_panel: ClonePanel,
    recent_list: RecentList,
    repo_overview: RepoOverviewPanel,
    stage_panel: StagePanel,
    history_panel: HistoryPanel,
    branches_panel: BranchPanel,
    auth_panel: AuthPanel,
    preferences_panel: PreferencesPanel,

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
        let logging = *config.logging();
        let default_clone_path = preferences.default_clone_path().to_string();
        preferences.standard.apply();
        colony_ui::i18n::set_locale(preferences.language().locale());
        let theme = Theme::active();
        let auth_manager =
            AuthManager::with_encrypted_fallback(preferences.allow_encrypted_tokens());
        let last_repo = config.last_repo().map(|entry| entry.path.clone());

        let mut app = Self {
            sidebar: Sidebar::default(),
            statuses: HashMap::new(),
            status_refresh: StatusRefresh::default(),
            clone_panel: ClonePanel::new(default_clone_path, preferences.network().clone()),
            recent_list: RecentList::new(),
            repo_overview: RepoOverviewPanel::new(preferences.network().clone()),
            history_panel: HistoryPanel::new(),
            branches_panel: BranchPanel::new(preferences.pinned_branches().to_vec()),
            stage_panel: StagePanel::new(),
            auth_panel: AuthPanel::new(auth_manager.clone()),
            preferences_panel: PreferencesPanel::new(preferences, logging),
            theme,
            config,
            current_repo: None,
            pending_repo: None,
            auth_manager,
            active_tab: MainTab::Clone,
            tab_order: MainTab::ALL.to_vec(),
            toasts: Toasts::default(),
        };

        if let Some(error) = migration_error {
            app.toasts.push(Toast::error(
                t("startup_migration_failed"),
                tf("startup_migration_failed_detail", &[("error", &error)]),
            ));
        }

        let accounts = app.auth_panel.refresh_accounts().map(Message::Auth);
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
            // ── Layout / navigation ────────────────────────────────────
            Message::Layout(LayoutMessage::TabSelected(tab)) => {
                self.active_tab = tab;
                Task::none()
            }

            Message::Sidebar(msg) => self.update_sidebar(msg),

            // ── Clone ──────────────────────────────────────────────────
            Message::Clone(msg) => {
                self.notify_clone_failure(&msg);
                let (task, cloned_path) = self.clone_panel.update(msg, &self.auth_manager);
                let load_task = if let Some(path) = cloned_path {
                    self.load_repo_context_async(path.to_string_lossy().to_string())
                } else {
                    Task::none()
                };
                Task::batch([task.map(Message::Clone), load_task])
            }

            // ── Recent ─────────────────────────────────────────────────
            Message::Recent(msg) => {
                let (task, selected) = self.recent_list.update(msg);
                let load_task = if let Some(path) = selected {
                    self.load_repo_context_async(path)
                } else {
                    Task::none()
                };
                Task::batch([task.map(Message::Recent), load_task])
            }

            // ── Repo overview ──────────────────────────────────────────
            Message::RepoOverview(msg) => {
                self.notify_auto_fetch_failure(&msg);
                let mut pulled = false;
                let finished = matches!(msg, repo_overview::Message::ActionFinished { .. });
                if let repo_overview::Message::ActionFinished {
                    path,
                    action,
                    auto: false,
                    result,
                } = &msg
                {
                    let current = self.current_repo.as_ref().map(|repo| repo.path());
                    if current != Some(&**path) {
                        // The panel only shows results for the open repository.
                        let title = format!("{} sur {path}", action.label());
                        self.toasts.push(match result {
                            Ok(msg) => Toast::success(format!("{title} : {msg}")),
                            Err(err) => Toast::error(title, format!("Échec : {err}")),
                        });
                    } else {
                        pulled =
                            matches!(action, repo_overview::RemoteAction::Pull) && result.is_ok();
                    }
                }
                let task = self
                    .repo_overview
                    .update(msg, self.current_repo.as_ref(), &self.auth_manager)
                    .map(Message::RepoOverview);
                let reload = if pulled {
                    self.reload_other_panels(MainTab::RepoOverview)
                } else {
                    Task::none()
                };
                let statuses = if finished {
                    self.refresh_statuses()
                } else {
                    Task::none()
                };
                Task::batch([task, reload, statuses])
            }

            // ── Stage ──────────────────────────────────────────────────
            Message::Stage(msg) => {
                // A commit moves HEAD: the other panels show the old state until reloaded.
                let changed = matches!(&msg, stage::Message::Committed(_, Ok(_)));
                let acted = changed || matches!(&msg, stage::Message::ActionDone(_, Ok(_)));
                let task = self
                    .stage_panel
                    .update(msg, self.current_repo.as_ref())
                    .map(Message::Stage);
                let reload = if changed {
                    self.reload_other_panels(MainTab::Stage)
                } else {
                    Task::none()
                };
                let statuses = if acted {
                    self.refresh_statuses()
                } else {
                    Task::none()
                };
                Task::batch([task, reload, statuses])
            }

            // ── History ────────────────────────────────────────────────
            Message::History(msg) => {
                let task = self.history_panel.update(msg, self.current_repo.as_ref());
                task.map(Message::History)
            }

            // ── Branches ───────────────────────────────────────────────
            Message::Branches(msg) => {
                // Checkout, merge and rebase change the working tree and HEAD.
                let changed = matches!(
                    &msg,
                    branches::Message::ActionDone(_, Ok(_)) | branches::Message::MergeDone(_, _)
                );
                let mut tasks = vec![self
                    .branches_panel
                    .update(msg, self.current_repo.as_ref())
                    .map(Message::Branches)];
                if changed {
                    tasks.push(self.reload_other_panels(MainTab::Branches));
                    tasks.push(self.refresh_statuses());
                }

                // Gerer la demande de navigation vers l'historique d'une branche
                if let Some(branch) = self.branches_panel.take_history_request() {
                    self.active_tab = MainTab::History;
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
                    // Sinon une validation ulterieure des preferences restaurerait l'ancienne liste.
                    self.preferences_panel
                        .set_preferences(self.config.preferences().clone());
                    self.save_config();
                }

                Task::batch(tasks)
            }

            // ── Auth ───────────────────────────────────────────────────
            Message::Auth(msg) => {
                let task = self.auth_panel.update(msg);
                task.map(Message::Auth)
            }

            // ── Preferences ────────────────────────────────────────────
            Message::Preferences(msg) => {
                let panel_task = self.preferences_panel.update(msg);
                let extra_task = self.handle_preferences_changes();
                Task::batch([panel_task.map(Message::Preferences), extra_task])
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
                let tasks = [
                    self.repo_overview.refresh(&repo).map(Message::RepoOverview),
                    self.stage_panel.load(&repo).map(Message::Stage),
                    self.branches_panel.load(&repo).map(Message::Branches),
                    self.history_panel.load(&repo).map(Message::History),
                ];
                self.current_repo = Some(repo);
                Task::batch(tasks)
            }

            // ── Auto-fetch ─────────────────────────────────────────────
            Message::AutoFetchTick => {
                Task::batch([self.maybe_start_auto_fetch(), self.refresh_statuses()])
            }

            Message::TabShortcut(index) => {
                if let Some(tab) = REPO_TABS.get(index) {
                    self.active_tab = *tab;
                }
                Task::none()
            }

            Message::Escape => {
                if self.sidebar.is_searching() {
                    self.sidebar.close_search();
                } else if self.sidebar.is_menu_open() {
                    self.sidebar.set_menu(None);
                } else if self.preferences_panel.is_open() {
                    self.preferences_panel.toggle();
                }
                Task::none()
            }

            Message::WindowFocused => self.refresh_statuses(),

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
                if let Err(err) = repo_overview::open_folder(&path) {
                    self.toasts.push(Toast::error(
                        t("open_folder_failed"),
                        format!("{path}: {err}"),
                    ));
                }
                Task::none()
            }
            Side::OpenTerminal(path) => {
                self.sidebar.set_menu(None);
                if let Err(err) = repo_overview::open_terminal(&path) {
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
                if self.preferences_panel.is_open() {
                    self.preferences_panel.toggle();
                }
                self.active_tab = MainTab::Clone;
                Task::none()
            }
            Side::TogglePreferences => {
                self.preferences_panel.toggle();
                Task::none()
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
        self.sidebar.close_search();
        self.sidebar.set_menu(None);
        if self.current_repo.as_ref().map(RepoContext::path) == Some(path) {
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

    /// Reloads every repository panel except `origin` after it changed the repository.
    fn reload_other_panels(&mut self, origin: MainTab) -> Task<Message> {
        let Some(repo) = self.current_repo.clone() else {
            return Task::none();
        };
        let mut tasks = Vec::new();
        if origin != MainTab::RepoOverview {
            tasks.push(
                self.repo_overview
                    .reload_repo_state(&repo)
                    .map(Message::RepoOverview),
            );
        }
        if origin != MainTab::Stage {
            tasks.push(self.stage_panel.load(&repo).map(Message::Stage));
        }
        if origin != MainTab::Branches {
            tasks.push(self.branches_panel.load(&repo).map(Message::Branches));
        }
        tasks.push(self.history_panel.load(&repo).map(Message::History));
        Task::batch(tasks)
    }

    // ─── View ──────────────────────────────────────────────────────────

    /// Compose la vue complete de l'application.
    pub fn view(&self) -> Element<'_, Message> {
        let theme = &self.theme;

        let sidebar = self
            .sidebar
            .view(
                self.config.repos(),
                &self.statuses,
                self.current_repo.as_ref().map(RepoContext::path),
                self.preferences_panel.is_open(),
            )
            .map(Message::Sidebar);

        // Preferences take the place of the tabs while open.
        let main_area: Element<'_, Message> = match self.preferences_panel.view(theme) {
            Some(preferences) => container(preferences.map(Message::Preferences))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            None => {
                let tab_bar = layout::view_tab_bar(theme, &self.tab_order, self.active_tab)
                    .map(Message::Layout);
                column![tab_bar, self.view_active_tab(theme)]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(iced::Padding {
                        top: 0.0,
                        right: 16.0,
                        bottom: 16.0,
                        left: 16.0,
                    })
                    .into()
            }
        };

        let toast_overlay = self.toast_overlay();
        let page = row![sidebar, main_area].height(Length::Fill);

        // Utiliser un container pour le fond + overlay notifications
        let base = container(page)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(theme.palette.background.into()),
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

    /// Rendu de l'onglet actif.
    fn view_active_tab<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        match self.active_tab {
            MainTab::Clone => self.clone_panel.view(theme).map(Message::Clone),
            MainTab::Open => self
                .recent_list
                .view(theme, &self.config)
                .map(Message::Recent),
            MainTab::RepoOverview => self
                .repo_overview
                .view(theme, self.current_repo.as_ref())
                .map(Message::RepoOverview),
            MainTab::Stage => self
                .stage_panel
                .view(theme, self.current_repo.as_ref())
                .map(Message::Stage),
            MainTab::History => self
                .history_panel
                .view(theme, self.current_repo.as_ref())
                .map(Message::History),
            MainTab::Branches => self
                .branches_panel
                .view(theme, self.current_repo.as_ref())
                .map(Message::Branches),
            MainTab::Auth => self.auth_panel.view(theme).map(Message::Auth),
        }
    }

    fn toast_overlay(&self) -> Element<'_, Message> {
        self.toasts
            .view(Message::ToastDismissed, Message::ToastAction)
    }

    // ─── Theme ─────────────────────────────────────────────────────────

    /// Retourne le theme Iced natif derive de notre theme custom.
    pub fn theme(&self) -> iced::Theme {
        self.theme.to_iced_theme()
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

        // Timer auto-fetch. Il est recree apres chaque operation du panneau, donc le prochain
        // auto-fetch part un intervalle apres la derniere operation reseau ou le dernier chargement.
        if self.config.preferences().auto_fetch_enabled()
            && self.current_repo.is_some()
            && !self.repo_overview.is_busy()
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
            self.toasts.push(Toast::error(
                "Erreur de sauvegarde",
                format!("Impossible de sauvegarder la configuration : {err}"),
            ));
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

    /// Applique les changements de preferences depuis le panneau preferences.
    fn handle_preferences_changes(&mut self) -> Task<Message> {
        let mut task = Task::none();
        if let Some(prefs) = self.preferences_panel.take_changes() {
            task = self.apply_preferences(prefs);
        }
        if let Some(logging) = self.preferences_panel.take_logging_changes() {
            self.apply_logging(logging);
        }
        task
    }

    /// Applique un jeu de preferences mis a jour a toute l'application.
    fn apply_preferences(&mut self, preferences: Preferences) -> Task<Message> {
        let old = self.config.preferences();
        let rescan = old.scan_clone_folder() != preferences.scan_clone_folder()
            || old.default_clone_path() != preferences.default_clone_path();
        self.config.set_preferences(preferences.clone());
        preferences.standard.apply();
        colony_ui::i18n::set_locale(preferences.language().locale());
        self.theme = Theme::active();

        // Propager aux panneaux
        self.clone_panel
            .set_default_destination(preferences.default_clone_path().to_string());
        self.clone_panel
            .set_network_preferences(preferences.network().clone());
        self.repo_overview
            .set_network_preferences(preferences.network().clone());
        self.branches_panel
            .set_pinned_branches(preferences.pinned_branches().to_vec());
        self.auth_manager
            .set_encrypted_fallback(preferences.allow_encrypted_tokens());
        let accounts = self
            .auth_panel
            .set_auth_manager(self.auth_manager.clone())
            .map(Message::Auth);
        self.preferences_panel.set_preferences(preferences);

        self.save_config();
        let scan = if rescan {
            self.rescan_clone_folder()
        } else {
            Task::none()
        };
        Task::batch([accounts, scan])
    }

    /// Applique les options de logging mises a jour.
    fn apply_logging(&mut self, logging: LoggingOptions) {
        self.config.set_logging(logging);
        self.save_config();
    }

    fn handle_toast_action(&mut self, action: ToastAction) -> Task<Message> {
        match action {
            ToastAction::Retry => {
                let (task, _) = self
                    .clone_panel
                    .update(clone::Message::RetryClone, &self.auth_manager);
                task.map(Message::Clone)
            }
            ToastAction::CopyLogPath(path) => iced::clipboard::write(path.display().to_string()),
        }
    }

    /// Signale un échec de clonage ou de recherche, avec le chemin des logs.
    fn notify_clone_failure(&mut self, message: &clone::Message) {
        let (title, err, can_retry) = match message {
            clone::Message::CloneResult(Err(err)) => ("Échec du clonage", err, true),
            clone::Message::SearchResult(version, Err(err))
                if self.clone_panel.is_current_search(*version) =>
            {
                ("Échec de la recherche", err, false)
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

    /// Signale l'echec d'un auto-fetch sur le depot courant.
    fn notify_auto_fetch_failure(&mut self, message: &repo_overview::Message) {
        if let repo_overview::Message::ActionFinished {
            path,
            auto: true,
            result: Err(err),
            ..
        } = message
        {
            if self.current_repo.as_ref().map(RepoContext::path) == Some(&**path) {
                self.toasts
                    .push(Toast::error("Auto-fetch echoue", err.clone()));
            }
        }
    }

    /// Demarre l'auto-fetch du depot courant si l'option est active.
    fn maybe_start_auto_fetch(&mut self) -> Task<Message> {
        if !self.config.preferences().auto_fetch_enabled() {
            return Task::none();
        }
        let Some(repo) = self.current_repo.as_ref() else {
            return Task::none();
        };
        self.repo_overview
            .auto_fetch(repo, &self.auth_manager)
            .map(Message::RepoOverview)
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
                    .filter(|digit| (1..=REPO_TABS.len()).contains(digit))
                    .map(|digit| Message::TabShortcut(digit - 1)),
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(message: Option<Message>) -> String {
        message.map_or_else(|| "None".to_string(), |message| format!("{message:?}"))
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
