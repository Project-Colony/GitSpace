//! Application principale GitSpace — architecture Elm pour Iced 0.13.
//!
//! Centralise le modèle, les messages, le dispatch update/view,
//! les subscriptions (auto-fetch, notifications, clavier) et le thème.

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{column, container, row};
use iced::{Element, Length, Subscription, Task};

use crate::auth::AuthManager;
use crate::config::{AppConfig, LoggingOptions, Preferences};
use crate::git::remote::fetch_remote;
use crate::ui::{
    auth::AuthPanel,
    branches::BranchPanel,
    clone::ClonePanel,
    context::RepoContext,
    dev_gallery::DevGalleryPanel,
    history::HistoryPanel,
    layout::{self, LayoutMessage, MainTab},
    notifications::{Notification, NotificationAction, NotificationCenter},
    preferences::PreferencesPanel,
    recent::RecentList,
    repo_overview::RepoOverviewPanel,
    stage::StagePanel,
    theme::Theme,
};
use crate::update;

// ─── Message principal ─────────────────────────────────────────────────────

/// Message racine de l'application, enveloppe tous les sous-messages.
#[derive(Debug, Clone)]
pub enum Message {
    // Layout / navigation
    Layout(LayoutMessage),

    // Panneaux
    Clone(crate::ui::clone::Message),
    Recent(crate::ui::recent::Message),
    RepoOverview(crate::ui::repo_overview::Message),
    Stage(crate::ui::stage::Message),
    History(crate::ui::history::Message),
    Branches(crate::ui::branches::Message),
    Auth(crate::ui::auth::Message),
    Preferences(crate::ui::preferences::Message),
    DevGallery(crate::ui::dev_gallery::Message),

    // Notifications
    Notification(crate::ui::notifications::Message),

    // Chargement async du contexte d'un depot
    RepoContextLoaded(String, Option<RepoContext>),

    // Mises a jour automatiques
    UpdateCheckResult(update::UpdateResult),

    // Auto-fetch
    AutoFetchResult(AutoFetchOutcome),

    // Tick de nettoyage notifications
    NotificationTick,

    // Auto-fetch timer tick
    AutoFetchTick(Instant),
}

// ─── Modele principal ──────────────────────────────────────────────────────

/// Etat global de l'application GitSpace.
pub struct GitSpaceApp {
    theme: Theme,
    active_tab: MainTab,
    tab_order: Vec<MainTab>,

    // Panneaux
    clone_panel: ClonePanel,
    recent_list: RecentList,
    repo_overview: RepoOverviewPanel,
    stage_panel: StagePanel,
    history_panel: HistoryPanel,
    branches_panel: BranchPanel,
    auth_panel: AuthPanel,
    preferences_panel: PreferencesPanel,
    dev_gallery_panel: DevGalleryPanel,

    // Etat global
    config: AppConfig,
    current_repo: Option<RepoContext>,
    auth_manager: AuthManager,
    notifications: NotificationCenter,

    // Mise a jour
    update_checked: bool,
    update_checking: bool,

    // Auto-fetch
    auto_fetch_last: Option<Instant>,
    auto_fetch_in_progress: bool,
    auto_fetch_repo: Option<Arc<str>>,
}

impl GitSpaceApp {
    /// Construit l'etat initial et retourne les taches de demarrage.
    pub fn new() -> (Self, Task<Message>) {
        let config = AppConfig::load();
        let preferences = config.preferences().clone();
        let logging = config.logging().clone();
        let default_clone_path = preferences.default_clone_path().to_string();
        let theme = Theme::from_mode(preferences.theme_mode());
        let auth_manager =
            AuthManager::with_encrypted_fallback(preferences.allow_encrypted_tokens());
        let current_repo = config
            .recent_repos()
            .first()
            .and_then(|entry| RepoContext::from_path(&entry.path));

        let tab_order = {
            let mut tabs = MainTab::ALL.to_vec();
            if !cfg!(debug_assertions) {
                tabs.retain(|tab| *tab != MainTab::DevGallery);
            }
            tabs
        };

        let app = Self {
            clone_panel: ClonePanel::new(
                default_clone_path,
                preferences.network().clone(),
            ),
            recent_list: RecentList::new(),
            repo_overview: RepoOverviewPanel::new(preferences.network().clone()),
            history_panel: HistoryPanel::new(),
            branches_panel: BranchPanel::new(preferences.pinned_branches().to_vec()),
            stage_panel: StagePanel::new(),
            auth_panel: AuthPanel::new(auth_manager.clone()),
            preferences_panel: PreferencesPanel::new(preferences, logging),
            dev_gallery_panel: DevGalleryPanel::new(),
            theme,
            config,
            current_repo,
            auth_manager,
            active_tab: MainTab::Clone,
            tab_order,
            notifications: NotificationCenter::new(),
            update_checked: false,
            update_checking: false,
            auto_fetch_last: None,
            auto_fetch_in_progress: false,
            auto_fetch_repo: None,
        };

        // Verifier les mises a jour au demarrage si active
        let startup_task = if app.config.preferences().auto_check_updates() {
            trigger_update_check(&app.config)
        } else {
            Task::none()
        };

        (app, startup_task)
    }

    // ─── Update ────────────────────────────────────────────────────────

    /// Dispatch principal des messages.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Layout / navigation ────────────────────────────────────
            Message::Layout(layout_msg) => {
                match layout_msg {
                    LayoutMessage::TabSelected(tab) => {
                        self.active_tab = tab;
                    }
                    LayoutMessage::HeaderClicked => {
                        self.preferences_panel.toggle();
                    }
                    LayoutMessage::SidebarNav(tab) => {
                        self.active_tab = tab;
                    }
                    LayoutMessage::OpenFileManager => {
                        if let Some(repo) = &self.current_repo {
                            let _ = open::that(repo.path());
                        }
                    }
                    LayoutMessage::CopyPath => {
                        // Copie geree par le systeme d'exploitation via arboard
                        if let Some(repo) = &self.current_repo {
                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                let _ = clipboard.set_text(repo.path());
                            }
                        }
                    }
                }
                Task::none()
            }

            // ── Clone ──────────────────────────────────────────────────
            Message::Clone(msg) => {
                let (task, cloned_path) =
                    self.clone_panel.update(msg, &self.auth_manager);
                let load_task = if let Some(path) = cloned_path {
                    self.load_repo_context_async(path.to_string_lossy().to_string())
                } else {
                    Task::none()
                };
                Task::batch([task.map(Message::Clone), load_task])
            }

            // ── Recent ─────────────────────────────────────────────────
            Message::Recent(msg) => {
                let (task, selected) =
                    self.recent_list.update(msg);
                let load_task = if let Some(path) = selected {
                    self.load_repo_context_async(path)
                } else {
                    Task::none()
                };
                Task::batch([task.map(Message::Recent), load_task])
            }

            // ── Repo overview ──────────────────────────────────────────
            Message::RepoOverview(msg) => {
                let task = self.repo_overview.update(
                    msg,
                    self.current_repo.as_ref(),
                    &self.auth_manager,
                );
                task.map(Message::RepoOverview)
            }

            // ── Stage ──────────────────────────────────────────────────
            Message::Stage(msg) => {
                let task = self.stage_panel.update(msg, self.current_repo.as_ref());
                task.map(Message::Stage)
            }

            // ── History ────────────────────────────────────────────────
            Message::History(msg) => {
                let task = self.history_panel.update(msg, self.current_repo.as_ref());
                task.map(Message::History)
            }

            // ── Branches ───────────────────────────────────────────────
            Message::Branches(msg) => {
                let task = self.branches_panel.update(msg, self.current_repo.as_ref());

                // Gerer la demande de navigation vers l'historique d'une branche
                if let Some(branch) = self.branches_panel.take_history_request() {
                    self.active_tab = MainTab::History;
                    self.history_panel
                        .set_branch_filter(branch, self.current_repo.as_ref());
                }

                // Persister les branches epinglees
                if let Some(pinned) = self.branches_panel.take_pinned_changes() {
                    let mut prefs = self.config.preferences().clone();
                    prefs.set_pinned_branches(pinned);
                    self.config.set_preferences(prefs);
                    self.save_config();
                }

                task.map(Message::Branches)
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

            // ── Dev gallery ────────────────────────────────────────────
            Message::DevGallery(msg) => {
                let task = self.dev_gallery_panel.update(msg);
                task.map(Message::DevGallery)
            }

            // ── Notifications ──────────────────────────────────────────
            Message::Notification(msg) => {
                let actions = self.notifications.update(msg);
                self.handle_notification_actions(actions);
                Task::none()
            }

            Message::NotificationTick => {
                self.notifications.update(crate::ui::notifications::Message::Tick);
                Task::none()
            }

            // ── Chargement async du contexte depot ──────────────────────
            Message::RepoContextLoaded(path, context) => {
                self.current_repo = context;
                if self.config.touch_recent(std::path::Path::new(&path)) {
                    self.save_config();
                }
                if let Some(repo) = &self.current_repo {
                    self.repo_overview.refresh(repo);
                    self.stage_panel.refresh_if_needed(repo);
                }
                Task::none()
            }

            // ── Update check ───────────────────────────────────────────
            Message::UpdateCheckResult(result) => {
                self.update_checking = false;
                self.update_checked = true;
                self.handle_update_result(result);
                Task::none()
            }

            // ── Auto-fetch ─────────────────────────────────────────────
            Message::AutoFetchTick(_now) => {
                self.maybe_start_auto_fetch()
            }

            Message::AutoFetchResult(outcome) => {
                self.auto_fetch_in_progress = false;
                self.handle_auto_fetch_result(outcome);
                Task::none()
            }
        }
    }

    // ─── View ──────────────────────────────────────────────────────────

    /// Compose la vue complete de l'application.
    pub fn view(&self) -> Element<'_, Message> {
        let theme = &self.theme;

        // Si le panneau de preferences est ouvert, afficher uniquement celui-ci
        if self.preferences_panel.is_open() {
            if let Some(prefs_view) = self.preferences_panel.view(theme) {
                let overlay = self.notification_overlay(theme);
                return column![
                    prefs_view.map(Message::Preferences),
                    overlay,
                ]
                .into();
            }
        }

        // Header
        let header = layout::view_header(theme).map(Message::Layout);

        // Sidebar
        let sidebar = layout::view_sidebar(theme, self.active_tab, self.current_repo.as_ref())
            .map(Message::Layout);

        // Barre d'onglets
        let tab_bar = layout::view_tab_bar(theme, &self.tab_order, self.active_tab)
            .map(Message::Layout);

        // Contenu de l'onglet actif
        let content = self.view_active_tab(theme);

        // Overlay notifications
        let notification_overlay = self.notification_overlay(theme);

        // Assemblage : header en haut, puis sidebar + (tabs + content) en dessous
        let main_area = column![
            tab_bar,
            content,
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(iced::Padding { top: 0.0, right: 16.0, bottom: 16.0, left: 16.0 });

        let body = row![
            sidebar,
            main_area,
        ]
        .height(Length::Fill);

        // Superposer les notifications en haut a droite
        let page = column![
            header,
            body,
        ]
        .height(Length::Fill);

        // Utiliser un container pour le fond + overlay notifications
        let base = container(page)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(theme.palette.background.into()),
                ..Default::default()
            });

        if self.notifications.is_empty() {
            base.into()
        } else {
            // Placer les notifications en overlay via une colonne empilee
            column![
                iced::widget::stack![
                    base,
                    container(notification_overlay)
                        .width(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Right)
                        .padding(iced::Padding { top: 60.0, right: 16.0, bottom: 0.0, left: 0.0 }),
                ]
                .width(Length::Fill)
                .height(Length::Fill),
            ]
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
            MainTab::DevGallery => self.dev_gallery_panel.view(theme).map(Message::DevGallery),
        }
    }

    /// Overlay des notifications toast.
    fn notification_overlay<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        self.notifications.view(theme).map(Message::Notification)
    }

    // ─── Theme ─────────────────────────────────────────────────────────

    /// Retourne le theme Iced natif derive de notre theme custom.
    pub fn theme(&self) -> iced::Theme {
        self.theme.to_iced_theme()
    }

    // ─── Subscriptions ─────────────────────────────────────────────────

    /// Abonnements actifs : tick notifications + timer auto-fetch.
    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = Vec::new();

        // Tick pour le dismiss automatique des notifications
        if self.notifications.has_visible() {
            subs.push(
                iced::time::every(Duration::from_secs(1)).map(|_| Message::NotificationTick),
            );
        }

        // Timer auto-fetch
        if self.config.preferences().auto_fetch_enabled()
            && self.current_repo.is_some()
            && !self.auto_fetch_in_progress
        {
            let interval = Duration::from_secs(
                self.config.preferences().auto_fetch_interval_minutes() * 60,
            );
            subs.push(iced::time::every(interval).map(Message::AutoFetchTick));
        }

        Subscription::batch(subs)
    }

    // ─── Helpers prives ────────────────────────────────────────────────

    /// Sauvegarde la configuration et notifie l'utilisateur en cas d'echec.
    fn save_config(&mut self) {
        if let Err(err) = self.config.save() {
            self.notifications.push(Notification::error(
                "Erreur de sauvegarde",
                format!("Impossible de sauvegarder la configuration : {err}"),
            ));
        }
    }

    /// Lance le chargement async du contexte d'un depot.
    /// Les I/O git sont deplacees dans un thread bloquant pour ne pas
    /// bloquer le thread UI.
    fn load_repo_context_async(&self, path: String) -> Task<Message> {
        Task::perform(
            async move {
                let p = path.clone();
                let context = tokio::task::spawn_blocking(move || {
                    RepoContext::from_path(&p)
                })
                .await
                .ok()
                .flatten();
                (path, context)
            },
            |(path, context)| Message::RepoContextLoaded(path, context),
        )
    }

    /// Applique les changements de preferences depuis le panneau preferences.
    /// Retourne une Task si une verification de mise a jour est demandee.
    fn handle_preferences_changes(&mut self) -> Task<Message> {
        if let Some(prefs) = self.preferences_panel.take_changes() {
            self.apply_preferences(prefs);
        }
        if let Some(logging) = self.preferences_panel.take_logging_changes() {
            self.apply_logging(logging);
        }
        if self.preferences_panel.take_update_request() && !self.update_checking {
            self.update_checking = true;
            return trigger_update_check(&self.config);
        }
        Task::none()
    }

    /// Applique un jeu de preferences mis a jour a toute l'application.
    fn apply_preferences(&mut self, preferences: Preferences) {
        self.config.set_preferences(preferences.clone());
        self.theme = Theme::from_mode(preferences.theme_mode());

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
        self.auth_panel.set_auth_manager(self.auth_manager.clone());
        self.preferences_panel.set_preferences(preferences);

        self.save_config();
        self.update_checked = false;
    }

    /// Applique les options de logging mises a jour.
    fn apply_logging(&mut self, logging: LoggingOptions) {
        self.config.set_logging(logging);
        self.save_config();
    }

    /// Traite le resultat d'une verification de mise a jour.
    fn handle_update_result(&mut self, result: update::UpdateResult) {
        match result {
            Ok(Some(release)) => {
                let mut notification = Notification::success(
                    format!("Mise a jour {} disponible", release.version),
                    format!(
                        "Une version {:?} est prete au telechargement.",
                        release.channel
                    ),
                );
                notification.detail = release.notes.clone();
                notification =
                    notification.with_action(NotificationAction::OpenRelease(release.url.clone()));
                self.notifications.push(notification);

                let status = format!(
                    "Mise a jour {} disponible ({:?})",
                    release.version, release.channel
                );
                self.preferences_panel.set_update_status(&status);
            }
            Ok(None) => {
                let status = "Vous utilisez la derniere version.";
                self.preferences_panel.set_update_status(status);
            }
            Err(err) => {
                let status = format!("Erreur de verification: {err}");
                self.preferences_panel.set_update_status(&status);
                self.notifications
                    .push(Notification::error("Erreur de mise a jour", err.to_string()));
            }
        }
    }

    /// Traite les actions declenchees depuis les notifications.
    fn handle_notification_actions(&mut self, actions: Vec<NotificationAction>) {
        for action in actions {
            match action {
                NotificationAction::RetryClone => {
                    // Re-declenchement du clone via le panneau
                }
                NotificationAction::CopyLogPath(path) => {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(path.display().to_string());
                    }
                }
                NotificationAction::OpenRelease(url) => {
                    let _ = open::that(&url);
                }
            }
        }
    }

    /// Demarre l'auto-fetch si les conditions sont remplies.
    fn maybe_start_auto_fetch(&mut self) -> Task<Message> {
        if self.auto_fetch_in_progress {
            return Task::none();
        }

        let preferences = self.config.preferences();
        if !preferences.auto_fetch_enabled() {
            return Task::none();
        }

        let Some(repo) = self.current_repo.as_ref() else {
            return Task::none();
        };

        // Reinitialiser si le depot a change
        if self.auto_fetch_repo.as_deref() != Some(repo.path()) {
            self.auto_fetch_repo = Some(repo.path_arc());
            self.auto_fetch_last = Some(Instant::now());
            return Task::none();
        }

        let context = match self.repo_overview.auto_fetch_context(repo, &self.auth_manager) {
            Ok(ctx) => ctx,
            Err(err) => {
                self.repo_overview
                    .set_action_status(Some(format!("Auto-fetch echoue: {err}")));
                self.notifications
                    .push(Notification::error("Auto-fetch echoue", err));
                return Task::none();
            }
        };

        self.auto_fetch_in_progress = true;
        self.auto_fetch_last = Some(Instant::now());
        self.repo_overview
            .set_action_status(Some(format!("Auto-fetch {}...", context.remote_name)));

        let repo_path = context.repo_path.clone();
        let remote_name = context.remote_name.clone();
        let token = context.token.clone();
        let network = context.network.clone();

        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let result = fetch_remote(&repo_path, &remote_name, &network, token)
                        .map(|_| ())
                        .map_err(|err| err.to_string());
                    AutoFetchOutcome {
                        repo_path,
                        remote_name,
                        result,
                    }
                })
                .await
                .unwrap_or_else(|err| AutoFetchOutcome {
                    repo_path: String::new(),
                    remote_name: String::new(),
                    result: Err(err.to_string()),
                })
            },
            Message::AutoFetchResult,
        )
    }

    /// Traite le resultat d'un auto-fetch.
    fn handle_auto_fetch_result(&mut self, outcome: AutoFetchOutcome) {
        match outcome.result {
            Ok(()) => {
                if let Some(repo) = &self.current_repo {
                    if repo.path() == outcome.repo_path {
                        self.repo_overview.reload_repo_state(repo);
                    }
                }
                self.repo_overview.set_action_status(Some(format!(
                    "Auto-fetch {} termine",
                    outcome.remote_name
                )));
            }
            Err(err) => {
                self.repo_overview.set_action_status(Some(format!(
                    "Auto-fetch echoue: {err}"
                )));
                self.notifications.push(Notification::error(
                    "Auto-fetch echoue",
                    format!("{} ({})", err, outcome.remote_name),
                ));
            }
        }
    }
}

// ─── Helpers hors impl ─────────────────────────────────────────────────────

/// Lance une verification de mise a jour en tache asynchrone.
fn trigger_update_check(config: &AppConfig) -> Task<Message> {
    let channel = config.preferences().release_channel();
    let feed_override = config
        .preferences()
        .update_feed_override()
        .map(str::to_string);
    let network = config.preferences().network().clone();

    Task::perform(
        async move {
            update::check_for_updates(channel, feed_override.as_deref(), &network).await
        },
        Message::UpdateCheckResult,
    )
}

/// Resultat d'une operation d'auto-fetch.
#[derive(Debug, Clone)]
pub struct AutoFetchOutcome {
    pub repo_path: String,
    pub remote_name: String,
    pub result: Result<(), String>,
}
