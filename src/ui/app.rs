//! Application principale GitSpace — architecture Elm pour Iced 0.13.
//!
//! Centralise le modèle, les messages, le dispatch update/view,
//! les subscriptions (auto-fetch, notifications, clavier) et le thème.

use std::time::Duration;

use iced::widget::{column, container, row};
use iced::{Element, Length, Subscription, Task};

use crate::auth::AuthManager;
use crate::config::{AppConfig, LoggingOptions, Preferences};
use crate::error::logs_directory;
use crate::ui::{
    auth::AuthPanel,
    branches::BranchPanel,
    clone::{self, ClonePanel},
    context::RepoContext,
    history::HistoryPanel,
    layout::{self, LayoutMessage, MainTab},
    notifications::{Notification, NotificationAction, NotificationCenter},
    preferences::PreferencesPanel,
    recent::RecentList,
    repo_overview::{self, RepoOverviewPanel},
    stage::StagePanel,
    theme::Theme,
};

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

    // Notifications
    Notification(crate::ui::notifications::Message),

    // Chargement async du contexte d'un depot
    RepoContextLoaded(String, Option<RepoContext>),

    // Tick de nettoyage notifications
    NotificationTick,

    // Auto-fetch timer tick
    AutoFetchTick,
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

    // Etat global
    config: AppConfig,
    current_repo: Option<RepoContext>,
    /// Dernier depot demande : seul son resultat de chargement est applique.
    pending_repo: Option<String>,
    auth_manager: AuthManager,
    notifications: NotificationCenter,
}

impl GitSpaceApp {
    /// Construit l'etat initial et retourne les taches de demarrage.
    pub fn new() -> (Self, Task<Message>) {
        let config = AppConfig::load();
        let preferences = config.preferences().clone();
        let logging = *config.logging();
        let default_clone_path = preferences.default_clone_path().to_string();
        let theme = Theme::from_mode(preferences.theme_mode());
        let auth_manager =
            AuthManager::with_encrypted_fallback(preferences.allow_encrypted_tokens());
        let last_repo = config
            .recent_repos()
            .first()
            .map(|entry| entry.path.clone());

        let mut app = Self {
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
            notifications: NotificationCenter::new(),
        };

        let accounts = app.auth_panel.refresh_accounts().map(Message::Auth);
        let repo = match last_repo {
            Some(path) => app.load_repo_context_async(path),
            None => Task::none(),
        };
        (app, Task::batch([accounts, repo]))
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
                    LayoutMessage::OpenFileManager => {
                        if let Some(repo) = &self.current_repo {
                            if let Err(err) = repo_overview::open_folder(repo.path()) {
                                self.notifications.push(Notification::error(
                                    "Impossible d'ouvrir le dossier",
                                    format!("{} : {err}", repo.path()),
                                ));
                            }
                        }
                    }
                    LayoutMessage::CopyPath => {
                        if let Some(repo) = &self.current_repo {
                            return iced::clipboard::write(repo.path().to_string());
                        }
                    }
                }
                Task::none()
            }

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
                let task =
                    self.repo_overview
                        .update(msg, self.current_repo.as_ref(), &self.auth_manager);
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
                    // Sinon une validation ulterieure des preferences restaurerait l'ancienne liste.
                    self.preferences_panel
                        .set_preferences(self.config.preferences().clone());
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

            // ── Notifications ──────────────────────────────────────────
            Message::Notification(msg) => {
                let actions = self.notifications.update(msg);
                self.handle_notification_actions(actions)
            }

            Message::NotificationTick => {
                self.notifications
                    .update(crate::ui::notifications::Message::Tick);
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
                    self.notifications.push(Notification::error(
                        "Impossible d'ouvrir le dépôt",
                        format!("{path} n'est pas un dépôt Git lisible."),
                    ));
                    return Task::none();
                };
                if self.config.touch_recent(std::path::Path::new(&path)) {
                    self.save_config();
                }
                let task = self.repo_overview.refresh(&repo);
                self.stage_panel.refresh_if_needed(&repo);
                self.current_repo = Some(repo);
                task.map(Message::RepoOverview)
            }

            // ── Auto-fetch ─────────────────────────────────────────────
            Message::AutoFetchTick => self.maybe_start_auto_fetch(),
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
                return column![prefs_view.map(Message::Preferences), overlay,].into();
            }
        }

        // Header
        let header = layout::view_header(theme).map(Message::Layout);

        // Sidebar
        let sidebar = layout::view_sidebar(theme, self.active_tab, self.current_repo.as_ref())
            .map(Message::Layout);

        // Barre d'onglets
        let tab_bar =
            layout::view_tab_bar(theme, &self.tab_order, self.active_tab).map(Message::Layout);

        // Contenu de l'onglet actif
        let content = self.view_active_tab(theme);

        // Overlay notifications
        let notification_overlay = self.notification_overlay(theme);

        // Assemblage : header en haut, puis sidebar + (tabs + content) en dessous
        let main_area = column![tab_bar, content,]
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(iced::Padding {
                top: 0.0,
                right: 16.0,
                bottom: 16.0,
                left: 16.0,
            });

        let body = row![sidebar, main_area,].height(Length::Fill);

        // Superposer les notifications en haut a droite
        let page = column![header, body,].height(Length::Fill);

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
            column![iced::widget::stack![
                base,
                container(notification_overlay)
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
        if !self.notifications.is_empty() {
            subs.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::NotificationTick));
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
    fn load_repo_context_async(&mut self, path: String) -> Task<Message> {
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
        let accounts = self
            .auth_panel
            .set_auth_manager(self.auth_manager.clone())
            .map(Message::Auth);
        self.preferences_panel.set_preferences(preferences);

        self.save_config();
        accounts
    }

    /// Applique les options de logging mises a jour.
    fn apply_logging(&mut self, logging: LoggingOptions) {
        self.config.set_logging(logging);
        self.save_config();
    }

    /// Traite les actions declenchees depuis les notifications.
    fn handle_notification_actions(&mut self, actions: Vec<NotificationAction>) -> Task<Message> {
        let mut tasks = Vec::new();
        for action in actions {
            match action {
                NotificationAction::RetryClone => {
                    let (task, _) = self
                        .clone_panel
                        .update(clone::Message::RetryClone, &self.auth_manager);
                    tasks.push(task.map(Message::Clone));
                }
                NotificationAction::CopyLogPath(path) => {
                    tasks.push(iced::clipboard::write(path.display().to_string()));
                }
            }
        }
        Task::batch(tasks)
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
        let mut notification = Notification::error(title, err.user_message());
        if can_retry {
            notification = notification.with_action(NotificationAction::RetryClone);
        }
        notification = notification.with_action(NotificationAction::CopyLogPath(logs_directory()));
        notification.detail = Some(err.detail().to_string());
        self.notifications.push(notification);
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
                self.notifications
                    .push(Notification::error("Auto-fetch echoue", err.clone()));
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
