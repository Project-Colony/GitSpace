//! Structure de la coquille de l'application : header, sidebar, tab bar.

use iced::widget::{button, column, container, horizontal_rule, row, scrollable, text, Space};
use iced::{Alignment, Element, Length};

use crate::ui::context::RepoContext;
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
    DevGallery,
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
            Self::DevGallery => "Dev Gallery",
        }
    }
}

/// Messages générés par le layout shell.
#[derive(Debug, Clone)]
pub enum LayoutMessage {
    TabSelected(MainTab),
    HeaderClicked,
    SidebarNav(MainTab),
    OpenFileManager,
    CopyPath,
}

/// Construit le header de l'application.
pub fn view_header(theme: &Theme) -> Element<'_, LayoutMessage> {
    let header_content = row![
        Space::with_width(8),
        button(
            text("GitSpace")
                .size(theme.typography.heading)
                .color(theme.palette.text_primary)
        )
        .on_press(LayoutMessage::HeaderClicked)
        .padding([4, 8])
        .style(|_: &iced::Theme, _| button::Style {
            background: None,
            ..Default::default()
        }),
        Space::with_width(12),
        text("Workspace shell")
            .size(theme.typography.body)
            .color(theme.palette.accent),
        Space::with_width(Length::Fill),
    ]
    .align_y(Alignment::Center)
    .height(48);

    container(header_content)
        .width(Length::Fill)
        .style(move |_: &iced::Theme| {
            let surface = theme.palette.surface;
            container::Style {
                background: Some(surface.into()),
                ..Default::default()
            }
        })
        .into()
}

/// Construit la sidebar de navigation.
pub fn view_sidebar<'a>(
    theme: &'a Theme,
    active_tab: MainTab,
    repo: Option<&'a RepoContext>,
) -> Element<'a, LayoutMessage> {
    let mut sidebar = column![].spacing(2).padding(8).width(220);

    sidebar = sidebar.push(Space::with_height(12));
    sidebar = sidebar.push(
        text("Navigation")
            .size(theme.typography.title)
            .color(theme.palette.text_primary),
    );
    sidebar = sidebar.push(horizontal_rule(1));
    sidebar = sidebar.push(Space::with_height(8));

    // Section Workspaces
    sidebar = sidebar.push(
        text("Workspaces")
            .size(theme.typography.label)
            .color(theme.palette.text_secondary),
    );
    for (label, tab) in [
        ("Recent", MainTab::Open),
        ("Favorites", MainTab::Open),
        ("Local Repos", MainTab::Open),
        ("Remote Repos", MainTab::Clone),
    ] {
        sidebar = sidebar.push(menu::menu_item(
            theme,
            label,
            active_tab == tab,
            LayoutMessage::SidebarNav(tab),
        ));
    }

    sidebar = sidebar.push(Space::with_height(12));
    sidebar = sidebar.push(
        text("Actions")
            .size(theme.typography.label)
            .color(theme.palette.text_secondary),
    );
    for (label, tab) in [
        ("Clone", MainTab::Clone),
        ("Open", MainTab::Open),
        ("New Branch", MainTab::Branches),
        ("Sync", MainTab::Stage),
    ] {
        sidebar = sidebar.push(menu::menu_item(
            theme,
            label,
            active_tab == tab,
            LayoutMessage::SidebarNav(tab),
        ));
    }

    // Section contexte
    sidebar = sidebar.push(Space::with_height(12));
    sidebar = sidebar.push(
        text("Context")
            .size(theme.typography.label)
            .color(theme.palette.text_secondary),
    );

    if let Some(repo) = repo {
        sidebar = sidebar.push(
            text("Active repository")
                .size(theme.typography.label)
                .color(theme.palette.text_secondary),
        );
        sidebar = sidebar.push(
            text(&repo.name)
                .size(theme.typography.body)
                .color(theme.palette.text_primary),
        );
        sidebar = sidebar.push(
            text(repo.path())
                .size(theme.typography.label)
                .color(theme.palette.text_secondary),
        );
        sidebar = sidebar.push(Space::with_height(8));
        sidebar = sidebar.push(
            button(text("Ouvrir le dossier").size(12))
                .on_press(LayoutMessage::OpenFileManager)
                .padding([2, 8]),
        );
        sidebar = sidebar.push(
            button(text("Copier le chemin").size(12))
                .on_press(LayoutMessage::CopyPath)
                .padding([2, 8]),
        );
    } else {
        sidebar = sidebar.push(
            text("Aucun dépôt sélectionné")
                .size(theme.typography.label)
                .color(theme.palette.text_secondary),
        );
    }

    let sidebar_surface = theme.palette.surface;
    let sidebar_border = theme.palette.surface_highlight;

    container(scrollable(sidebar).height(Length::Fill))
        .width(220)
        .height(Length::Fill)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(sidebar_surface.into()),
            border: iced::Border {
                width: 1.0,
                color: sidebar_border,
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
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

    column![
        container(tabs).width(Length::Fill),
        horizontal_rule(1),
    ]
    .into()
}
