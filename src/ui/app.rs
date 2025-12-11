use eframe::egui;

use crate::config::AppConfig;
use crate::ui::{
    clone::ClonePanel,
    context::RepoContext,
    layout::{MainTab, ShellLayout},
    recent::RecentList,
    repo_overview::RepoOverviewPanel,
    theme::Theme,
};

pub struct GitSpaceApp {
    theme: Theme,
    initialized: bool,
    active_tab: MainTab,
    clone_panel: ClonePanel,
    recent_list: RecentList,
    repo_overview: RepoOverviewPanel,
    config: AppConfig,
    current_repo: Option<RepoContext>,
}

impl GitSpaceApp {
    pub fn new() -> Self {
        let theme = Theme::dark();
        let config = AppConfig::load();
        let current_repo = config
            .recent_repos()
            .first()
            .map(|entry| RepoContext::from_path(&entry.path));
        Self {
            clone_panel: ClonePanel::new(theme.clone()),
            recent_list: RecentList::new(theme.clone()),
            repo_overview: RepoOverviewPanel::new(theme.clone()),
            config,
            current_repo,
            theme,
            initialized: false,
            active_tab: MainTab::Clone,
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
                self.current_repo.as_ref(),
            ) {
                self.load_repo_context(selected);
            }
        });

        if let Some(cloned_path) = self.clone_panel.take_last_cloned_repo() {
            self.load_repo_context(cloned_path);
        }
    }
}
