use eframe::egui::{self, Align, Layout, RichText, Sense, Ui};

use crate::ui::theme::Theme;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MainTab {
    Clone,
    Open,
    RepoOverview,
    History,
    Branches,
}

impl MainTab {
    pub const ALL: [Self; 5] = [
        Self::Clone,
        Self::Open,
        Self::RepoOverview,
        Self::History,
        Self::Branches,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Clone => "Clone",
            Self::Open => "Open",
            Self::RepoOverview => "Repo Overview",
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

    pub fn right_panel(&self, ctx: &egui::Context) {
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
                ui.label(
                    RichText::new(
                        "Resizable sidebars keep the layout responsive across window sizes.",
                    )
                    .color(self.theme.palette.text_secondary),
                );
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

    pub fn tab_content(&self, ui: &mut Ui, tab: MainTab) {
        let body_color = self.theme.palette.text_secondary;
        ui.add_space(8.0);
        match tab {
            MainTab::Clone => {
                ui.heading(
                    RichText::new("Clone a repository").color(self.theme.palette.text_primary),
                );
                ui.label(
                    RichText::new(
                        "Provide a URL or select a template to clone into your workspace.",
                    )
                    .color(body_color),
                );
            }
            MainTab::Open => {
                ui.heading(
                    RichText::new("Open a repository").color(self.theme.palette.text_primary),
                );
                ui.label(
                    RichText::new("Browse your filesystem to open existing projects.")
                        .color(body_color),
                );
            }
            MainTab::RepoOverview => {
                ui.heading(
                    RichText::new("Repository overview").color(self.theme.palette.text_primary),
                );
                ui.label(
                    RichText::new("Insights, README preview, and key metrics will appear here.")
                        .color(body_color),
                );
            }
            MainTab::History => {
                ui.heading(RichText::new("Commit history").color(self.theme.palette.text_primary));
                ui.label(RichText::new("Visualize commit graphs and timelines.").color(body_color));
            }
            MainTab::Branches => {
                ui.heading(
                    RichText::new("Branch management").color(self.theme.palette.text_primary),
                );
                ui.label(RichText::new("Create, switch, and compare branches.").color(body_color));
            }
        }
    }
}
