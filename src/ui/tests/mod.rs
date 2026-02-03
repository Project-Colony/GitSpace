use std::sync::Arc;

use eframe::egui;

use crate::auth::AuthManager;
use crate::config::{AppConfig, LoggingOptions, Preferences};
use crate::ui::layout::{MainTab, ShellLayout};
use crate::ui::theme::{SharedTheme, Theme};
use crate::ui::{
    auth::AuthPanel, branches::BranchPanel, clone::ClonePanel, history::HistoryPanel,
    notifications::NotificationCenter, recent::RecentList, repo_overview::RepoOverviewPanel,
    settings::SettingsPanel, stage::StagePanel,
};

fn build_layout_components() -> (
    SharedTheme,
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
    let theme = Theme::shared_from_mode(crate::config::ThemeMode::Mocha);
    let preferences = Preferences::default();
    let logging = LoggingOptions::default();
    let auth_manager = AuthManager::default();
    (
        Arc::clone(&theme),
        ClonePanel::new(
            Arc::clone(&theme),
            preferences.default_clone_path().to_string(),
            preferences.network().clone(),
        ),
        RecentList::new(Arc::clone(&theme)),
        AppConfig::default(),
        RepoOverviewPanel::new(
            Arc::clone(&theme),
            preferences.branch_box_height(),
            preferences.network().clone(),
        ),
        StagePanel::new(Arc::clone(&theme)),
        HistoryPanel::new(Arc::clone(&theme)),
        BranchPanel::new(Arc::clone(&theme), preferences.pinned_branches().to_vec()),
        AuthPanel::new(Arc::clone(&theme), auth_manager.clone()),
        SettingsPanel::new(Arc::clone(&theme), preferences, logging),
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

    let layout = ShellLayout::new(Arc::clone(&theme));
    let mut active_tab = MainTab::Clone;
    let mut tab_order = MainTab::ALL.to_vec();

    let output = egui::Context::default().run(Default::default(), |ctx| {
        theme.apply(ctx);
        layout.header(ctx);
        layout.sidebar(ctx, active_tab, None);

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
                None,
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

    let layout = ShellLayout::new(Arc::clone(&theme));
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
                None,
            );
        });
    });

    assert!(output.textures_delta.free.is_empty());
}
