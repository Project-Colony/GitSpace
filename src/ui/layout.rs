//! The tab bar above the repository panels.

use iced::widget::{column, container, row, rule};
use iced::{Element, Length};

use crate::ui::menu;
use crate::ui::theme::Theme;

/// Onglet principal de l'application.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum MainTab {
    Clone,
    Open,
    RepoOverview,
    Stage,
    History,
    Branches,
    Auth,
}

impl MainTab {
    pub const ALL: [Self; 7] = [
        Self::Clone,
        Self::Open,
        Self::RepoOverview,
        Self::Stage,
        Self::History,
        Self::Branches,
        Self::Auth,
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
        }
    }
}

/// Messages générés par le layout shell.
#[derive(Debug, Clone)]
pub enum LayoutMessage {
    TabSelected(MainTab),
}

/// Construit la barre d'onglets.
pub fn view_tab_bar<'a>(
    theme: &'a Theme,
    tab_order: &'a [MainTab],
    active: MainTab,
) -> Element<'a, LayoutMessage> {
    let mut tabs = row![].spacing(4).padding([0, 8]);

    for tab in tab_order {
        tabs = tabs.push(menu::tab_button(
            theme,
            tab.label(),
            active == *tab,
            LayoutMessage::TabSelected(*tab),
        ));
    }

    column![container(tabs).width(Length::Fill), rule::horizontal(1),].into()
}
