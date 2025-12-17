use eframe::egui::{self, Align, Id, Layout, RichText, Sense, Ui};

use crate::auth::AuthManager;
use crate::config::AppConfig;
use crate::ui::{
    auth::AuthPanel, branches::BranchPanel, clone::ClonePanel, context::RepoContext,
    notifications::NotificationCenter, recent::RecentList, repo_overview::RepoOverviewPanel,
    settings::SettingsPanel, stage::StagePanel, theme::Theme,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MainTab {
    Clone,
    Open,
    RepoOverview,
    Stage,
    History,
    Branches,
    Auth,
    Settings,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NavigationTrigger {
    Click,
    Keyboard,
    ContextMenu,
    DragAndDrop,
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
    pub const ALL: [Self; 8] = [
        Self::Clone,
        Self::Open,
        Self::RepoOverview,
        Self::Stage,
        Self::History,
        Self::Branches,
        Self::Auth,
        Self::Settings,
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

    pub fn sidebar(&self, ctx: &egui::Context) {
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
                for label in ["Recent", "Favorites", "Local Repos", "Remote Repos"] {
                    ui.add_space(4.0);
                    ui.colored_label(self.theme.palette.text_primary, label);
                }

                ui.add_space(12.0);
                ui.label(RichText::new("Actions").color(self.theme.palette.text_secondary));
                for action in ["Clone", "Open", "New Branch", "Sync"] {
                    let response = ui.add(egui::SelectableLabel::new(
                        false,
                        RichText::new(action).strong(),
                    ));
                    if response.hovered() {
                        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                    }
                }
            });
    }

    pub fn right_panel(&self, ctx: &egui::Context, repo: Option<&RepoContext>) {
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
                } else {
                    ui.label(
                        RichText::new(
                            "Select a repository from Recent or finish cloning to load its context.",
                        )
                        .color(self.theme.palette.text_secondary),
                    );
                }
            });
    }

    pub fn tab_bar(
        &self,
        ui: &mut Ui,
        tab_order: &mut Vec<MainTab>,
        active: &mut MainTab,
    ) -> TabInteraction {
        let mut interaction = TabInteraction::default();
        let dragging_id = Id::new("main_tab_dragging");
        let mut dragging: Option<usize> = ui
            .ctx()
            .data_mut(|data| data.get_persisted(dragging_id))
            .unwrap_or(None);

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;

            let pointer_down = ui.input(|i| i.pointer.primary_down());
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

                let response = ui.add_sized(
                    [120.0, 32.0],
                    egui::Label::new(label).sense(Sense::click_and_drag()),
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
                    if ui.button(format!("Switch to {}", tab.label())).clicked() {
                        *active = tab;
                        interaction.selected = Some((tab, NavigationTrigger::ContextMenu));
                        ui.close_menu();
                    }
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
                tab_order.swap(from, to);
                dragging = Some(to);
                interaction.reordered = Some((from, to));
                interaction.selected = Some((tab_order[to], NavigationTrigger::DragAndDrop));
                *active = tab_order[to];
            }

            if !pointer_down {
                dragging = None;
            }
        });

        ui.ctx()
            .data_mut(|data| data.insert_persisted(dragging_id, dragging));

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
    ) -> Option<String> {
        ui.add_space(8.0);
        match tab {
            MainTab::Clone => {
                clone_panel.ui(ui, auth_manager, notifications);
                None
            }
            MainTab::Open => recent_list.ui(ui, config).map(|entry| entry.path),
            MainTab::RepoOverview => {
                repo_overview.ui(ui, repo);
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
                settings_panel.ui(ui);
                None
            }
        }
    }
}
