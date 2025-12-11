use eframe::egui;

use crate::auth::AuthManager;
use crate::config::{AppConfig, Preferences};
use crate::ui::{
    auth::AuthPanel,
    branches::BranchPanel,
    clone::ClonePanel,
    context::RepoContext,
    history::HistoryPanel,
    layout::{MainTab, ShellLayout},
    notifications::{NotificationAction, NotificationCenter},
    recent::RecentList,
    repo_overview::RepoOverviewPanel,
    settings::SettingsPanel,
    stage::StagePanel,
    theme::Theme,
};

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
    }
}

impl eframe::App for GitSpaceApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.initialize_if_needed(ctx);

        let theme = self.theme.clone();
        let layout = ShellLayout::new(&theme);
        layout.header(ctx);
        layout.sidebar(ctx);
        layout.right_panel(ctx, self.current_repo.as_ref());

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.set_min_height(ui.available_height());
            layout.tab_bar(ui, &mut self.active_tab);
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

        if let Some(updated_preferences) = self.settings_panel.take_changes() {
            self.apply_preferences(updated_preferences, ctx);
        }

        if let Some(cloned_path) = self.clone_panel.take_last_cloned_repo() {
            self.load_repo_context(cloned_path);
        }

        for action in self.notifications.show(ctx) {
            match action {
                NotificationAction::RetryClone => self.clone_panel.retry_last_clone(),
                NotificationAction::CopyLogPath(path) => {
                    ctx.output_mut(|o| o.copied_text = path.display().to_string());
                }
            }
        }
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
        self.settings_panel.set_preferences(preferences);

        let _ = self.config.save();
    }
}
