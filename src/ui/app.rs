use eframe::egui;
use poll_promise::Promise;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::auth::AuthManager;
use crate::config::{AppConfig, Preferences};
use crate::telemetry::TelemetryEmitter;
use crate::ui::{
    auth::AuthPanel,
    branches::BranchPanel,
    clone::ClonePanel,
    context::RepoContext,
    history::HistoryPanel,
    layout::{MainTab, ShellLayout},
    notifications::{Notification, NotificationAction, NotificationCenter},
    recent::RecentList,
    repo_overview::RepoOverviewPanel,
    settings::SettingsPanel,
    stage::StagePanel,
    theme::Theme,
};
use crate::update;

pub struct GitSpaceApp {
    theme: Theme,
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
    notifications: NotificationCenter,
    update_promise: Option<Promise<update::UpdateResult>>,
    update_checked: bool,
    telemetry: TelemetryEmitter,
    telemetry_prompt_enqueued: bool,
}

impl GitSpaceApp {
    pub fn new() -> Self {
        let config = AppConfig::load();
        let preferences = config.preferences().clone();
        let default_clone_path = preferences.default_clone_path().to_string();
        let theme = Theme::from_mode(preferences.theme_mode());
        let settings_theme = theme.clone();
        let auth_manager = AuthManager::new();
        let current_repo = config
            .recent_repos()
            .first()
            .map(|entry| RepoContext::from_path(&entry.path));
        let mut telemetry = TelemetryEmitter::new();
        telemetry.set_enabled(preferences.telemetry_enabled());
        if preferences.telemetry_enabled() {
            let mut properties = Map::new();
            properties.insert(
                "release_channel".to_string(),
                Value::String(format!("{:?}", preferences.release_channel())),
            );
            telemetry.record_event("app_launch", properties);
        }
        Self {
            clone_panel: ClonePanel::new(theme.clone(), default_clone_path),
            recent_list: RecentList::new(theme.clone()),
            repo_overview: RepoOverviewPanel::new(theme.clone()),
            history_panel: HistoryPanel::new(theme.clone()),
            branches_panel: BranchPanel::new(theme.clone()),
            stage_panel: StagePanel::new(theme.clone()),
            config,
            current_repo,
            auth_panel: AuthPanel::new(theme.clone(), auth_manager.clone()),
            auth_manager,
            theme,
            initialized: false,
            active_tab: MainTab::Clone,
            settings_panel: SettingsPanel::new(settings_theme, preferences),
            notifications: NotificationCenter::default(),
            update_promise: None,
            update_checked: false,
            telemetry,
            telemetry_prompt_enqueued: false,
        }
    }

    fn initialize_if_needed(&mut self, ctx: &egui::Context) {
        if !self.initialized {
            self.theme.apply(ctx);
            self.initialized = true;
        }
    }

    fn load_repo_context<P: AsRef<std::path::Path>>(&mut self, path: P) {
        let path_ref = path.as_ref();
        self.current_repo = Some(RepoContext::from_path(path_ref));
        if self.config.touch_recent(path_ref) {
            let _ = self.config.save();
        }

        if self.telemetry_enabled() {
            let mut properties = Map::new();
            let mut hasher = Sha256::new();
            hasher.update(path_ref.to_string_lossy().as_bytes());
            let hash = format!("{:x}", hasher.finalize());
            properties.insert("repo_hash".to_string(), Value::String(hash));
            self.telemetry.record_event("repo_opened", properties);
        }
    }
}

impl eframe::App for GitSpaceApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.initialize_if_needed(ctx);
        self.prompt_for_telemetry_if_needed();

        let theme = self.theme.clone();
        let layout = ShellLayout::new(&theme);
        layout.header(ctx);
        layout.sidebar(ctx);
        layout.right_panel(ctx, self.current_repo.as_ref());

        egui::CentralPanel::default().show(ctx, |ui| {
            layout.tab_bar(ui, &mut self.active_tab);
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
                    ) {
                        self.load_repo_context(selected);
                    }
                });
        });

        if let Some(updated_preferences) = self.settings_panel.take_changes() {
            self.apply_preferences(updated_preferences, ctx);
        }

        if self.settings_panel.take_telemetry_purge_request() {
            self.telemetry.purge();
            self.settings_panel
                .set_telemetry_status("Pending diagnostics cleared");
            self.notifications.push(Notification::success(
                "Telemetry cleared",
                "Queued and offline telemetry events were removed.",
            ));
        }

        if let Some(cloned_path) = self.clone_panel.take_last_cloned_repo() {
            self.load_repo_context(cloned_path);
        }

        if self.settings_panel.take_update_request() {
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
                NotificationAction::EnableTelemetry => {
                    self.enable_telemetry(ctx);
                }
                NotificationAction::DeclineTelemetry => {
                    self.decline_telemetry(ctx);
                }
            }
        }

        self.telemetry.tick();
    }
}

impl GitSpaceApp {
    fn apply_preferences(&mut self, preferences: Preferences, ctx: &egui::Context) {
        self.config.set_preferences(preferences.clone());
        self.theme = Theme::from_mode(preferences.theme_mode());
        self.theme.apply(ctx);

        self.clone_panel.set_theme(self.theme.clone());
        self.clone_panel
            .set_default_destination(preferences.default_clone_path().to_string());
        self.recent_list.set_theme(self.theme.clone());
        self.repo_overview.set_theme(self.theme.clone());
        self.history_panel.set_theme(self.theme.clone());
        self.branches_panel.set_theme(self.theme.clone());
        self.stage_panel.set_theme(self.theme.clone());
        self.auth_panel.set_theme(self.theme.clone());
        self.settings_panel.set_theme(self.theme.clone());
        self.settings_panel.set_preferences(preferences.clone());

        self.telemetry.set_enabled(preferences.telemetry_enabled());
        if self.telemetry_enabled() {
            let mut properties = Map::new();
            properties.insert(
                "auto_update".to_string(),
                Value::Bool(self.config.preferences().auto_check_updates()),
            );
            self.telemetry
                .record_event("preferences_updated", properties);
        }

        let _ = self.config.save();

        // Allow update settings to take effect immediately on the next frame.
        self.update_checked = false;
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

        self.settings_panel
            .set_update_status("Checking for updates...");

        self.update_promise = Some(Promise::spawn_thread("update-check", move || {
            update::check_for_updates(channel, feed_override.as_deref())
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
                self.settings_panel.set_update_status(format!(
                    "Update {} available on the {:?} channel",
                    release.version, release.channel
                ));
            }
            Ok(None) => {
                self.settings_panel
                    .set_update_status("You're already on the latest version.");
            }
            Err(err) => {
                self.settings_panel
                    .set_update_status(format!("Update check failed: {err}"));
                self.notifications
                    .push(Notification::error("Update check failed", err.to_string()));
            }
        }
    }

    fn telemetry_enabled(&self) -> bool {
        self.config.preferences().telemetry_enabled()
    }

    fn prompt_for_telemetry_if_needed(&mut self) {
        if self.config.telemetry_prompt_shown() || self.telemetry_prompt_enqueued {
            return;
        }

        let mut notification = Notification::success(
            "Help improve GitSpace",
            "Share anonymized diagnostics to guide future improvements.",
        );
        notification.detail = Some(
            "Telemetry is optional, batched, and stored locally when offline. You can clear it anytime.".to_string(),
        );
        notification = notification
            .with_action(NotificationAction::EnableTelemetry)
            .with_action(NotificationAction::DeclineTelemetry);
        self.notifications.push(notification);
        self.telemetry_prompt_enqueued = true;
    }

    fn enable_telemetry(&mut self, ctx: &egui::Context) {
        if self.telemetry_enabled() {
            return;
        }

        let mut preferences = self.config.preferences().clone();
        preferences.set_telemetry_enabled(true);
        self.config.mark_telemetry_prompt_shown();
        self.settings_panel.set_preferences(preferences.clone());
        self.apply_preferences(preferences, ctx);

        self.telemetry.record_event("telemetry_opt_in", Map::new());
        let _ = self.config.save();
    }

    fn decline_telemetry(&mut self, ctx: &egui::Context) {
        let mut preferences = self.config.preferences().clone();
        preferences.set_telemetry_enabled(false);
        self.config.mark_telemetry_prompt_shown();
        self.settings_panel.set_preferences(preferences.clone());
        self.apply_preferences(preferences, ctx);
    }
}
