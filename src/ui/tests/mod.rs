use eframe::egui;

use crate::auth::AuthManager;
use crate::config::{AppConfig, Preferences};
use crate::ui::layout::{MainTab, ShellLayout};
use crate::ui::theme::Theme;
use crate::ui::{
    auth::AuthPanel, branches::BranchPanel, clone::ClonePanel, history::HistoryPanel,
    notifications::NotificationCenter, recent::RecentList, repo_overview::RepoOverviewPanel,
    settings::SettingsPanel, stage::StagePanel,
};

fn build_layout_components() -> (
    Theme,
    ClonePanel,
    RecentList,
    AppConfig,
    RepoOverviewPanel,
    StagePanel,
    HistoryPanel,
    BranchPanel,
    AuthPanel,
    SettingsPanel,
    NotificationCenter,
    AuthManager,
) {
    let theme = Theme::dark();
    let preferences = Preferences::default();
    let auth_manager = AuthManager::default();
    (
        theme.clone(),
        ClonePanel::new(
            theme.clone(),
            preferences.default_clone_path().to_string(),
            preferences.network().clone(),
        ),
        RecentList::new(theme.clone()),
        AppConfig::default(),
        RepoOverviewPanel::new(theme.clone()),
        StagePanel::new(theme.clone()),
        HistoryPanel::new(theme.clone()),
        BranchPanel::new(theme.clone()),
        AuthPanel::new(theme.clone(), auth_manager.clone()),
        SettingsPanel::new(theme.clone(), preferences),
        NotificationCenter::default(),
        auth_manager,
    )
}

#[test]
fn layout_panels_render_without_panic() {
    let (
        theme,
        mut clone_panel,
        mut recent_list,
        config,
        mut repo_overview,
        mut stage_panel,
        mut history_panel,
        mut branch_panel,
        mut auth_panel,
        mut settings_panel,
        mut notifications,
        auth_manager,
    ) = build_layout_components();

    let layout = ShellLayout::new(&theme);
    let mut active_tab = MainTab::Clone;
    let mut tab_order = MainTab::ALL.to_vec();

    let output = egui::Context::default().run(Default::default(), |ctx| {
        theme.apply(ctx);
        layout.header(ctx);
        layout.sidebar(ctx, active_tab);
        layout.right_panel(ctx, None);

        egui::CentralPanel::default().show(ctx, |ui| {
            layout.tab_bar(ui, &mut tab_order, &mut active_tab);
            layout.tab_content(
                ui,
                active_tab,
                &mut clone_panel,
                &mut recent_list,
                &config,
                &mut repo_overview,
                &mut stage_panel,
                &mut history_panel,
                &mut branch_panel,
                &mut auth_panel,
                &mut settings_panel,
                &mut notifications,
                None,
                &auth_manager,
            );
        });
    });

    assert!(!output.shapes.is_empty());
}

#[test]
fn layout_switches_tabs_in_run_loop() {
    let (
        theme,
        mut clone_panel,
        mut recent_list,
        config,
        mut repo_overview,
        mut stage_panel,
        mut history_panel,
        mut branch_panel,
        mut auth_panel,
        mut settings_panel,
        mut notifications,
        auth_manager,
    ) = build_layout_components();

    let layout = ShellLayout::new(&theme);
    let mut active_tab = MainTab::History;
    let mut tab_order = MainTab::ALL.to_vec();

    let output = egui::Context::default().run(Default::default(), |ctx| {
        theme.apply(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            layout.tab_bar(ui, &mut tab_order, &mut active_tab);
            layout.tab_content(
                ui,
                active_tab,
                &mut clone_panel,
                &mut recent_list,
                &config,
                &mut repo_overview,
                &mut stage_panel,
                &mut history_panel,
                &mut branch_panel,
                &mut auth_panel,
                &mut settings_panel,
                &mut notifications,
                None,
                &auth_manager,
            );
        });
    });

    assert!(output.textures_delta.free.is_empty());
}
