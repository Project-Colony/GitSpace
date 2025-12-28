use eframe::egui::{self, Align, Id, Layout, RichText, Sense, Ui, Vec2};

use crate::auth::AuthManager;
use crate::config::AppConfig;
use crate::ui::{
    auth::AuthPanel, branches::BranchPanel, clone::ClonePanel, context::RepoContext, dev_gallery,
    menu, notifications::NotificationCenter, perf::PerfScope, recent::RecentList,
    repo_overview::RepoOverviewPanel, settings::SettingsPanel, stage::StagePanel, theme::Theme,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum MainTab {
    Clone,
    Open,
    RepoOverview,
    Stage,
    History,
    Branches,
    Auth,
    Settings,
    DevGallery,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NavigationTrigger {
    Click,
    Keyboard,
    ContextMenu,
    DragAndDrop,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct NavigationSelection {
    pub tab: MainTab,
    pub trigger: NavigationTrigger,
}

impl NavigationTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Click => "click",
            Self::Keyboard => "keyboard",
            Self::ContextMenu => "context_menu",
            Self::DragAndDrop => "drag_and_drop",
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TabInteraction {
    pub selected: Option<(MainTab, NavigationTrigger)>,
    pub reordered: Option<(usize, usize)>,
}

impl MainTab {
    pub const ALL: [Self; 9] = [
        Self::Clone,
        Self::Open,
        Self::RepoOverview,
        Self::Stage,
        Self::History,
        Self::Branches,
        Self::Auth,
        Self::Settings,
        Self::DevGallery,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Clone => "Clone",
            Self::Open => "Open",
            Self::RepoOverview => "Repo Overview",
            Self::Stage => "Stage",
            Self::History => "History",
            Self::Branches => "Branches",
            Self::Auth => "Auth",
            Self::Settings => "Settings",
            Self::DevGallery => "Dev Gallery",
        }
    }
}

pub struct ShellLayout<'a> {
    theme: &'a Theme,
}

impl<'a> ShellLayout<'a> {
    pub fn new(theme: &'a Theme) -> Self {
        Self { theme }
    }

    pub fn header(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .exact_height(48.0)
            .frame(egui::Frame::none().fill(self.theme.palette.surface))
            .show(ctx, |ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add_space(8.0);
                    ui.heading(
                        RichText::new("GitSpace")
                            .color(self.theme.palette.text_primary)
                            .strong(),
                    );
                    ui.colored_label(self.theme.palette.accent, RichText::new("Workspace shell"));
                });
            });
    }

    pub fn sidebar(&self, ctx: &egui::Context, active_tab: MainTab) -> Option<NavigationSelection> {
        let mut selection = None;
        egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(220.0)
            .frame(
                egui::Frame::none()
                    .fill(self.theme.palette.surface)
                    .stroke(egui::Stroke::new(1.0, self.theme.palette.surface_highlight)),
            )
            .show(ctx, |ui| {
                ui.add_space(12.0);
                ui.heading(RichText::new("Navigation").color(self.theme.palette.text_primary));
                ui.separator();

                ui.label(RichText::new("Workspaces").color(self.theme.palette.text_secondary));
                for (label, tab) in [
                    ("Recent", MainTab::Open),
                    ("Favorites", MainTab::Open),
                    ("Local Repos", MainTab::Open),
                    ("Remote Repos", MainTab::Clone),
                ] {
                    ui.add_space(4.0);
                    let response = menu::menu_item_sized(
                        ui,
                        self.theme,
                        ("sidebar-nav", label),
                        label,
                        active_tab == tab,
                        Vec2::new(ui.available_width(), ui.spacing().interact_size.y.max(28.0)),
                        Sense::click(),
                    );

                    if response.clicked() {
                        selection = Some(NavigationSelection {
                            tab,
                            trigger: NavigationTrigger::Click,
                        });
                    }
                }

                ui.add_space(12.0);
                ui.label(RichText::new("Actions").color(self.theme.palette.text_secondary));
                for (action, tab) in [
                    ("Clone", MainTab::Clone),
                    ("Open", MainTab::Open),
                    ("New Branch", MainTab::Branches),
                    ("Sync", MainTab::Stage),
                ] {
                    let response = menu::menu_item_sized(
                        ui,
                        self.theme,
                        ("sidebar-action", action),
                        RichText::new(action).strong(),
                        active_tab == tab,
                        Vec2::new(ui.available_width(), ui.spacing().interact_size.y.max(28.0)),
                        Sense::click(),
                    );

                    if response.clicked() {
                        selection = Some(NavigationSelection {
                            tab,
                            trigger: NavigationTrigger::Click,
                        });
                    }
                }
            });
        selection
    }

    pub fn right_panel(
        &self,
        ctx: &egui::Context,
        repo: Option<&RepoContext>,
    ) -> Option<NavigationSelection> {
        let mut selection = None;
        egui::SidePanel::right("context")
            .resizable(true)
            .default_width(260.0)
            .frame(
                egui::Frame::none()
                    .fill(self.theme.palette.surface)
                    .stroke(egui::Stroke::new(1.0, self.theme.palette.surface_highlight)),
            )
            .show(ctx, |ui| {
                ui.add_space(12.0);
                ui.heading(RichText::new("Context").color(self.theme.palette.text_primary));
                ui.separator();
                if let Some(repo) = repo {
                    ui.label(
                        RichText::new("Active repository")
                            .color(self.theme.palette.text_secondary),
                    );
                    ui.label(
                        RichText::new(&repo.name)
                            .color(self.theme.palette.text_primary)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(&repo.path)
                            .color(self.theme.palette.text_secondary)
                            .italics(),
                    );

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Open in file manager").clicked() {
                            if let Err(err) = open::that(&repo.path) {
                                tracing::warn!("Failed to open repo path: {err}");
                            }
                        }

                        if ui.button("Copy path").clicked() {
                            ui.output_mut(|o| o.copied_text = repo.path.clone());
                        }
                    });

                    ui.horizontal(|ui| {
                        if ui.button("Switch repository").clicked() {
                            selection = Some(NavigationSelection {
                                tab: MainTab::Open,
                                trigger: NavigationTrigger::Click,
                            });
                        }

                        if ui.button("Branch view").clicked() {
                            selection = Some(NavigationSelection {
                                tab: MainTab::Branches,
                                trigger: NavigationTrigger::Click,
                            });
                        }
                    });
                } else {
                    ui.label(
                        RichText::new(
                            "Select a repository from Recent or finish cloning to load its context.",
                        )
                        .color(self.theme.palette.text_secondary),
                    );
                }
            });
        selection
    }

    pub fn tab_bar(
        &self,
        ui: &mut Ui,
        tab_order: &mut Vec<MainTab>,
        active: &mut MainTab,
    ) -> TabInteraction {
        let _scope = PerfScope::new("layout::tab_bar");
        let mut interaction = TabInteraction::default();
        let dragging_id = Id::new("main_tab_dragging");
        let swap_time_id = Id::new("main_tab_swap_time");
        let mut dragging: Option<usize> = ui
            .ctx()
            .data_mut(|data| data.get_persisted(dragging_id))
            .unwrap_or(None);
        let mut last_swap_time: f64 = ui
            .ctx()
            .data_mut(|data| data.get_persisted(swap_time_id))
            .unwrap_or(0.0);
        let swap_throttle_seconds = 0.08;

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;

            let (pointer_down, now) = ui.input(|i| (i.pointer.primary_down(), i.time));
            let mut hover_swap: Option<(usize, usize)> = None;

            for (index, tab) in tab_order.iter().copied().enumerate() {
                let is_active = *active == tab;
                let label = RichText::new(tab.label())
                    .color(if is_active {
                        self.theme.palette.text_primary
                    } else {
                        self.theme.palette.text_secondary
                    })
                    .strong();

                let response = menu::menu_item_sized(
                    ui,
                    self.theme,
                    ("tab-bar", tab),
                    label,
                    is_active,
                    Vec2::new(120.0, 32.0),
                    Sense::click_and_drag(),
                );

                if is_active {
                    let rect = response.rect;
                    let stroke = egui::Stroke::new(2.0, self.theme.palette.accent);
                    ui.painter()
                        .line_segment([rect.left_bottom(), rect.right_bottom()], stroke);
                }

                if response.clicked() {
                    *active = tab;
                    interaction.selected = Some((tab, NavigationTrigger::Click));
                }

                response.context_menu(|ui| {
                    menu::with_menu_popup_motion(ui, ("tab-menu", tab), |ui| {
                        if menu::menu_item(
                            ui,
                            self.theme,
                            ("tab-menu-switch", tab),
                            format!("Switch to {}", tab.label()),
                            is_active,
                        )
                        .clicked()
                        {
                            *active = tab;
                            interaction.selected = Some((tab, NavigationTrigger::ContextMenu));
                            ui.close_menu();
                        }
                    });
                });

                if response.drag_started() {
                    dragging = Some(index);
                }

                if let Some(dragging_index) = dragging {
                    if dragging_index != index && response.hovered() && pointer_down {
                        hover_swap = Some((dragging_index, index));
                    }
                }
            }

            if let Some((from, to)) = hover_swap {
                if now - last_swap_time < swap_throttle_seconds {
                    return;
                }
                tab_order.swap(from, to);
                dragging = Some(to);
                interaction.reordered = Some((from, to));
                interaction.selected = Some((tab_order[to], NavigationTrigger::DragAndDrop));
                *active = tab_order[to];
                last_swap_time = now;
            }

            if !pointer_down {
                dragging = None;
            }
        });

        ui.ctx()
            .data_mut(|data| data.insert_persisted(dragging_id, dragging));
        ui.ctx()
            .data_mut(|data| data.insert_persisted(swap_time_id, last_swap_time));

        ui.add_space(4.0);
        ui.separator();

        interaction
    }

    pub fn tab_content(
        &self,
        ui: &mut Ui,
        tab: MainTab,
        clone_panel: &mut ClonePanel,
        recent_list: &mut RecentList,
        config: &AppConfig,
        repo_overview: &mut RepoOverviewPanel,
        stage_panel: &mut StagePanel,
        history_panel: &mut crate::ui::history::HistoryPanel,
        branch_panel: &mut BranchPanel,
        auth_panel: &mut AuthPanel,
        settings_panel: &mut SettingsPanel,
        notifications: &mut NotificationCenter,
        repo: Option<&RepoContext>,
        auth_manager: &AuthManager,
        dev_gallery_panel: Option<&mut dev_gallery::DevGalleryPanel>,
    ) -> Option<String> {
        ui.add_space(8.0);
        match tab {
            MainTab::Clone => {
                clone_panel.ui(ui, auth_manager, notifications);
                None
            }
            MainTab::Open => recent_list.ui(ui, config),
            MainTab::RepoOverview => {
                repo_overview.ui(ui, repo, auth_manager);
                None
            }
            MainTab::Stage => {
                stage_panel.ui(ui, repo);
                None
            }
            MainTab::History => {
                history_panel.ui(ui, repo);
                None
            }
            MainTab::Branches => {
                branch_panel.ui(ui, repo);
                None
            }
            MainTab::Auth => {
                auth_panel.ui(ui);
                None
            }
            MainTab::Settings => {
                settings_panel.ui(ui, notifications);
                None
            }
            MainTab::DevGallery => {
                if let Some(panel) = dev_gallery_panel {
                    panel.ui(ui);
                } else {
                    ui.label("Dev gallery is only available in debug builds.");
                }
                None
            }
        }
    }
}
