use eframe::egui::{self, Align, Layout, RichText, Sense, Ui};

use crate::config::AppConfig;
use crate::ui::{
    branches::BranchPanel, clone::ClonePanel, context::RepoContext, recent::RecentList,
    repo_overview::RepoOverviewPanel, stage::StagePanel, theme::Theme,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MainTab {
    Clone,
    Open,
    RepoOverview,
    Stage,
    History,
    Branches,
}

impl MainTab {
    pub const ALL: [Self; 6] = [
        Self::Clone,
        Self::Open,
        Self::RepoOverview,
        Self::Stage,
        Self::History,
        Self::Branches,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Clone => "Clone",
            Self::Open => "Open",
            Self::RepoOverview => "Repo Overview",
            Self::Stage => "Stage",
            Self::History => "History",
            Self::Branches => "Branches",
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

    pub fn tab_bar(&self, ui: &mut Ui, active: &mut MainTab) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for tab in MainTab::ALL {
                let is_active = *active == tab;
                let label = RichText::new(tab.label())
                    .color(if is_active {
                        self.theme.palette.text_primary
                    } else {
                        self.theme.palette.text_secondary
                    })
                    .strong();

                let response =
                    ui.add_sized([120.0, 32.0], egui::Label::new(label).sense(Sense::click()));

                if is_active {
                    let rect = response.rect;
                    let stroke = egui::Stroke::new(2.0, self.theme.palette.accent);
                    ui.painter()
                        .line_segment([rect.left_bottom(), rect.right_bottom()], stroke);
                }

                if response.clicked() {
                    *active = tab;
                }
            }
        });
        ui.add_space(4.0);
        ui.separator();
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
        repo: Option<&RepoContext>,
    ) -> Option<String> {
        ui.add_space(8.0);
        match tab {
            MainTab::Clone => {
                clone_panel.ui(ui);
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
        }
    }
}
