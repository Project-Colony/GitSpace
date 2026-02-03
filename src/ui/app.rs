use std::sync::Arc;

use eframe::egui::{self, Key, Modifiers};
use poll_promise::Promise;

use crate::auth::AuthManager;
use crate::config::{AppConfig, Preferences};
use crate::git::remote::fetch_remote;
use crate::ui::{
    animation::store_motion_settings,
    auth::AuthPanel,
    branches::BranchPanel,
    clone::ClonePanel,
    context::RepoContext,
    fonts,
    history::HistoryPanel,
    layout::{MainTab, ShellLayout},
    notifications::{Notification, NotificationAction, NotificationCenter},
    preferences::PreferencesPanel,
    recent::RecentList,
    repo_overview::RepoOverviewPanel,
    settings::SettingsPanel,
    stage::StagePanel,
    dev_gallery::DevGalleryPanel,
    theme::{SharedTheme, Theme},
};
use crate::update;

pub struct GitSpaceApp {
    theme: SharedTheme,
    initialized: bool,
    active_tab: MainTab,
    clone_panel: ClonePanel,
    recent_list: RecentList,
    repo_overview: RepoOverviewPanel,
    history_panel: HistoryPanel,
    branches_panel: BranchPanel,
    stage_panel: StagePanel,
    config: AppConfig,
    current_repo: Option<RepoContext>,
    auth_manager: AuthManager,
    auth_panel: AuthPanel,
    settings_panel: SettingsPanel,
    preferences_panel: PreferencesPanel,
    dev_gallery_panel: DevGalleryPanel,
    notifications: NotificationCenter,
    update_promise: Option<Promise<update::UpdateResult>>,
    update_checked: bool,
    tab_order: Vec<MainTab>,
    auto_fetch_promise: Option<Promise<AutoFetchOutcome>>,
    auto_fetch_last_trigger: Option<f64>,
    auto_fetch_repo: Option<String>,
}

impl GitSpaceApp {
    pub fn new() -> Self {
        let config = AppConfig::load();
        let preferences = config.preferences().clone();
        let logging = config.logging().clone();
        let default_clone_path = preferences.default_clone_path().to_string();
        let theme = Theme::shared_from_mode(preferences.theme_mode());
        let auth_manager =
            AuthManager::with_encrypted_fallback(preferences.allow_encrypted_tokens());
        let current_repo = config
            .recent_repos()
            .first()
            .map(|entry| RepoContext::from_path(&entry.path));
        Self {
            clone_panel: ClonePanel::new(
                Arc::clone(&theme),
                default_clone_path,
                preferences.network().clone(),
            ),
            recent_list: RecentList::new(Arc::clone(&theme)),
            repo_overview: RepoOverviewPanel::new(
                Arc::clone(&theme),
                preferences.branch_box_height(),
                preferences.network().clone(),
            ),
            history_panel: HistoryPanel::new(Arc::clone(&theme)),
            branches_panel: BranchPanel::new(Arc::clone(&theme), preferences.pinned_branches().to_vec()),
            stage_panel: StagePanel::new(Arc::clone(&theme)),
            config,
            current_repo,
            auth_panel: AuthPanel::new(Arc::clone(&theme), auth_manager.clone()),
            auth_manager,
            settings_panel: SettingsPanel::new(Arc::clone(&theme), preferences.clone(), logging),
            preferences_panel: PreferencesPanel::new(Arc::clone(&theme), preferences, logging),
            dev_gallery_panel: DevGalleryPanel::new(Arc::clone(&theme)),
            theme,
            initialized: false,
            active_tab: MainTab::Clone,
            notifications: NotificationCenter::default(),
            update_promise: None,
            update_checked: false,
            tab_order: {
                let mut tabs = MainTab::ALL.to_vec();
                // Settings is now accessed via GitSpace header click
                tabs.retain(|tab| *tab != MainTab::Settings);
                if !cfg!(debug_assertions) {
                    tabs.retain(|tab| *tab != MainTab::DevGallery);
                }
                tabs
            },
            auto_fetch_promise: None,
            auto_fetch_last_trigger: None,
            auto_fetch_repo: None,
        }
    }

    fn initialize_if_needed(&mut self, ctx: &egui::Context) {
        if !self.initialized {
            fonts::install_fonts(ctx);
            let preferences = self.config.preferences().clone();
            self.apply_style_preferences(ctx, &preferences);
            self.initialized = true;
        }
    }

    fn load_repo_context<P: AsRef<std::path::Path>>(&mut self, path: P) {
        let path_ref = path.as_ref();
        self.current_repo = Some(RepoContext::from_path(path_ref));
        if self.config.touch_recent(path_ref) {
            let _ = self.config.save();
        }
    }
}

impl eframe::App for GitSpaceApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.initialize_if_needed(ctx);
        self.handle_keyboard_navigation(ctx);

        let layout = ShellLayout::new(Arc::clone(&self.theme));
        if layout.header(ctx) {
            self.preferences_panel.toggle();
        }

        // Show preferences panel fullscreen - if open, skip other panels
        let preferences_open = self.preferences_panel.show(ctx, &mut self.notifications);

        if !preferences_open {
            if let Some(selection) =
                layout.sidebar(ctx, self.active_tab, self.current_repo.as_ref())
            {
                if self.active_tab != selection.tab {
                    self.active_tab = selection.tab;
                }
            }

            egui::CentralPanel::default().show(ctx, |ui| {
                let _tab_interaction = layout.tab_bar(ui, &mut self.tab_order, &mut self.active_tab);
                let available_height = ui.available_height();
                egui::ScrollArea::vertical()
                    .id_source("main_tab_content")
                    .max_height(available_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if let Some(selected) = layout.tab_content(
                            ui,
                            self.active_tab,
                            &mut self.clone_panel,
                            &mut self.recent_list,
                            &self.config,
                            &mut self.repo_overview,
                            &mut self.stage_panel,
                            &mut self.history_panel,
                            &mut self.branches_panel,
                            &mut self.auth_panel,
                            &mut self.settings_panel,
                            &mut self.notifications,
                            self.current_repo.as_ref(),
                            &self.auth_manager,
                            Some(&mut self.dev_gallery_panel),
                        ) {
                            self.load_repo_context(selected);
                        }

                        if let Some(branch) = self.branches_panel.take_history_request() {
                            self.active_tab = MainTab::History;
                            self.history_panel
                                .set_branch_filter(branch, self.current_repo.as_ref());
                        }
                    });
            });
        }

        // Handle changes from both settings panel and preferences panel
        if let Some(updated_preferences) = self.settings_panel.take_changes() {
            self.apply_preferences(updated_preferences, ctx);
        }
        if let Some(updated_preferences) = self.preferences_panel.take_changes() {
            self.apply_preferences(updated_preferences, ctx);
        }

        if let Some(updated_logging) = self.settings_panel.take_logging_changes() {
            self.config.set_logging(updated_logging);
            let _ = self.config.save();
        }
        if let Some(updated_logging) = self.preferences_panel.take_logging_changes() {
            self.config.set_logging(updated_logging);
            let _ = self.config.save();
        }

        if let Some(pinned_branches) = self.branches_panel.take_pinned_changes() {
            let mut preferences = self.config.preferences().clone();
            preferences.set_pinned_branches(pinned_branches);
            self.config.set_preferences(preferences);
            let _ = self.config.save();
        }

        if let Some(control_height) = self.settings_panel.take_control_height_change() {
            self.apply_control_height(control_height, ctx);
        }
        if let Some(control_height) = self.preferences_panel.take_control_height_change() {
            self.apply_control_height(control_height, ctx);
        }

        if let Some(branch_height) = self.repo_overview.take_branch_box_height_change() {
            self.apply_branch_box_height(branch_height);
        }

        if let Some(cloned_path) = self.clone_panel.take_last_cloned_repo() {
            self.load_repo_context(cloned_path);
        }

        if self.settings_panel.take_update_request() {
            self.trigger_update_check();
        }
        if self.preferences_panel.take_update_request() {
            self.trigger_update_check();
        }

        if !self.update_checked && self.config.preferences().auto_check_updates() {
            self.trigger_update_check();
            self.update_checked = true;
        }

        if let Some(promise) = &self.update_promise {
            if let Some(result) = promise.ready() {
                self.handle_update_result(result.clone());
                self.update_promise = None;
            }
        }

        for action in self.notifications.show(ctx) {
            match action {
                NotificationAction::RetryClone => self.clone_panel.retry_last_clone(),
                NotificationAction::CopyLogPath(path) => {
                    ctx.output_mut(|o| o.copied_text = path.display().to_string());
                }
                NotificationAction::OpenRelease(url) => {
                    ctx.output_mut(|o| {
                        o.open_url = Some(egui::output::OpenUrl {
                            url: url.clone(),
                            new_tab: true,
                        });
                    });
                }
            }
        }

        self.handle_auto_fetch(ctx);
    }
}

impl GitSpaceApp {
    fn handle_keyboard_navigation(&mut self, ctx: &egui::Context) {
        let tab_order = self.tab_order.clone();
        let mut selected = None;

        ctx.input_mut(|input| {
            let mut command = Modifiers::default();
            command.command = true;

            let keys = [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
            ];

            for (index, key) in keys.into_iter().enumerate() {
                if input.consume_key(command, key) {
                    if let Some(tab) = tab_order.get(index) {
                        selected = Some(*tab);
                    }
                }
            }
        });

        if let Some(tab) = selected {
            if self.active_tab != tab {
                self.active_tab = tab;
            }
        }
    }

    fn apply_preferences(&mut self, preferences: Preferences, ctx: &egui::Context) {
        self.config.set_preferences(preferences.clone());
        self.theme = Theme::shared_from_mode(preferences.theme_mode());
        self.apply_style_preferences(ctx, &preferences);

        self.clone_panel.set_theme(Arc::clone(&self.theme));
        self.clone_panel
            .set_default_destination(preferences.default_clone_path().to_string());
        self.recent_list.set_theme(Arc::clone(&self.theme));
        self.repo_overview.set_theme(Arc::clone(&self.theme));
        self.repo_overview
            .set_branch_box_height(preferences.branch_box_height());
        self.repo_overview
            .set_network_preferences(preferences.network().clone());
        self.history_panel.set_theme(Arc::clone(&self.theme));
        self.branches_panel.set_theme(Arc::clone(&self.theme));
        self.branches_panel
            .set_pinned_branches(preferences.pinned_branches().to_vec());
        self.stage_panel.set_theme(Arc::clone(&self.theme));
        self.auth_panel.set_theme(Arc::clone(&self.theme));
        self.settings_panel.set_theme(Arc::clone(&self.theme));
        self.auth_manager
            .set_encrypted_fallback(preferences.allow_encrypted_tokens());
        self.auth_panel.set_auth_manager(self.auth_manager.clone());
        self.settings_panel.set_preferences(preferences.clone());
        self.preferences_panel.set_preferences(preferences.clone());
        self.preferences_panel.set_theme(Arc::clone(&self.theme));
        self.clone_panel
            .set_network_preferences(preferences.network().clone());

        let _ = self.config.save();

        // Allow update settings to take effect immediately on the next frame.
        self.update_checked = false;
    }

    fn apply_style_preferences(&self, ctx: &egui::Context, preferences: &Preferences) {
        self.theme.apply(ctx);
        let mut style = (*ctx.style()).clone();
        style.spacing.interact_size.y = preferences.control_height();
        ctx.set_style(style);
        store_motion_settings(ctx, preferences);
    }

    fn apply_control_height(&mut self, control_height: f32, ctx: &egui::Context) {
        let mut preferences = self.config.preferences().clone();
        if (preferences.control_height() - control_height).abs() <= f32::EPSILON {
            return;
        }
        preferences.set_control_height(control_height);
        self.config.set_preferences(preferences.clone());
        self.apply_style_preferences(ctx, &preferences);
        let _ = self.config.save();
    }

    fn apply_branch_box_height(&mut self, height: f32) {
        let mut preferences = self.config.preferences().clone();
        if (preferences.branch_box_height() - height).abs() <= f32::EPSILON {
            return;
        }
        preferences.set_branch_box_height(height);
        self.config.set_preferences(preferences.clone());
        self.repo_overview
            .set_branch_box_height(preferences.branch_box_height());
        let _ = self.config.save();
    }

    fn trigger_update_check(&mut self) {
        if self.update_promise.is_some() {
            return;
        }

        let channel = self.config.preferences().release_channel();
        let feed_override = self
            .config
            .preferences()
            .update_feed_override()
            .map(str::to_string);
        let network = self.config.preferences().network().clone();

        self.settings_panel
            .set_update_status("Checking for updates...");
        self.preferences_panel
            .set_update_status("Vérification des mises à jour...");

        self.update_promise = Some(Promise::spawn_thread("update-check", move || {
            update::check_for_updates(channel, feed_override.as_deref(), &network)
        }));
        self.update_checked = true;
    }

    fn handle_update_result(&mut self, result: update::UpdateResult) {
        match result {
            Ok(Some(release)) => {
                let mut notification = Notification::success(
                    format!("Update {} available", release.version),
                    format!(
                        "A {:?} channel build is ready to download.",
                        release.channel
                    ),
                );
                notification.detail = release.notes.clone();
                notification =
                    notification.with_action(NotificationAction::OpenRelease(release.url.clone()));
                self.notifications.push(notification);
                let status = format!(
                    "Update {} available on the {:?} channel",
                    release.version, release.channel
                );
                self.settings_panel.set_update_status(&status);
                self.preferences_panel.set_update_status(&status);
            }
            Ok(None) => {
                let status = "You're already on the latest version.";
                self.settings_panel.set_update_status(status);
                self.preferences_panel.set_update_status(status);
            }
            Err(err) => {
                let status = format!("Update check failed: {err}");
                self.settings_panel.set_update_status(&status);
                self.preferences_panel.set_update_status(&status);
                self.notifications
                    .push(Notification::error("Update check failed", err.to_string()));
            }
        }
    }

    fn handle_auto_fetch(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|input| input.time);

        if let Some(promise) = &self.auto_fetch_promise {
            if let Some(result) = promise.ready() {
                self.handle_auto_fetch_result(result.clone());
                self.auto_fetch_promise = None;
            }
        }

        if self.auto_fetch_promise.is_some() {
            return;
        }

        let preferences = self.config.preferences();
        if !preferences.auto_fetch_enabled() {
            return;
        }

        let Some(repo) = self.current_repo.as_ref() else {
            return;
        };

        let interval_secs = preferences.auto_fetch_interval_minutes() as f64 * 60.0;
        if self.auto_fetch_repo.as_deref() != Some(&repo.path) {
            self.auto_fetch_repo = Some(repo.path.clone());
            self.auto_fetch_last_trigger = Some(now - interval_secs);
        }

        let last_trigger = self.auto_fetch_last_trigger.unwrap_or(now - interval_secs);
        if now - last_trigger < interval_secs {
            return;
        }

        let context = match self.repo_overview.auto_fetch_context(repo, &self.auth_manager) {
            Ok(context) => context,
            Err(err) => {
                self.repo_overview
                    .set_action_status(Some(format!("Auto-fetch failed: {err}")));
                self.notifications
                    .push(Notification::error("Auto-fetch failed", err));
                self.auto_fetch_last_trigger = Some(now);
                return;
            }
        };

        let repo_path = context.repo_path.clone();
        let remote_name = context.remote_name.clone();
        let token = context.token.clone();
        let network = context.network.clone();

        self.repo_overview
            .set_action_status(Some(format!("Auto-fetching {remote_name}...")));

        self.auto_fetch_last_trigger = Some(now);
        self.auto_fetch_promise = Some(Promise::spawn_thread("auto-fetch", move || {
            let result = fetch_remote(&repo_path, &remote_name, &network, token)
                .map(|_| ())
                .map_err(|err| err.to_string());
            AutoFetchOutcome {
                repo_path,
                remote_name,
                result,
            }
        }));
    }

    fn handle_auto_fetch_result(&mut self, outcome: AutoFetchOutcome) {
        match outcome.result {
            Ok(()) => {
                if let Some(current_repo) = self.current_repo.as_ref() {
                    if current_repo.path == outcome.repo_path {
                        self.repo_overview.reload_repo_state(current_repo);
                    }
                }
                self.repo_overview.set_action_status(Some(format!(
                    "Auto-fetched {}",
                    outcome.remote_name
                )));
            }
            Err(err) => {
                self.repo_overview.set_action_status(Some(format!(
                    "Auto-fetch failed: {err}"
                )));
                self.notifications.push(Notification::error(
                    "Auto-fetch failed",
                    format!("{} ({})", err, outcome.remote_name),
                ));
            }
        }
    }
}

#[derive(Clone)]
struct AutoFetchOutcome {
    repo_path: String,
    remote_name: String,
    result: Result<(), String>,
}
