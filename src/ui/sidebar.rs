//! The left column: GitSpace's name, the repository list with its search, and the way to
//! add a repository.

use std::collections::HashMap;
use std::sync::Arc;

use colony_ui::theme::{contrast_on, Palette};
use colony_ui::widgets::{icons, identity_button, nav_hint, nav_label, selection_style};
use colony_ui::{fonts, Typography};
use iced::widget::text::Span;
use iced::widget::{
    button, column, container, hover, mouse_area, rich_text, row, scrollable, span, text,
    text_input, Column, Space,
};
use iced::{Alignment, Border, Element, Font, Length, Task};

use crate::config::RepoEntry;
use crate::git::status::QuickStatus;
use crate::i18n::{t, tf};
use crate::ui::repo_list::{self, Row};
use crate::ui::widgets::{button as gs_button, menu};

/// The search field, focused when the search opens.
const SEARCH_ID: &str = "sidebar-search"; // i18n: not user-visible

const SEARCH_GLYPH: &str = "\u{f002}";
const PIN_GLYPH: &str = "\u{f08d}";

#[derive(Debug, Clone)]
pub enum Message {
    ToggleSearch,
    Query(String),
    MoveSelection(i32),
    Activate,
    Select(Arc<str>),
    Pin(Arc<str>, bool),
    Hide(Arc<str>),
    OpenFolder(Arc<str>),
    OpenTerminal(Arc<str>),
    ContextMenu(Option<Arc<str>>),
    AddRepository,
    TogglePreferences,
    StatusesLoaded(HashMap<String, QuickStatus>),
}

#[derive(Debug, Default)]
pub struct Sidebar {
    searching: bool,
    query: String,
    /// The row Up/Down reached while searching; Enter opens it.
    cursor: Option<Arc<str>>,
    /// The repository whose right-click menu is open.
    menu: Option<Arc<str>>,
}

impl Sidebar {
    pub fn is_searching(&self) -> bool {
        self.searching
    }

    pub fn is_menu_open(&self) -> bool {
        self.menu.is_some()
    }

    pub fn set_menu(&mut self, path: Option<Arc<str>>) {
        self.menu = path;
    }

    /// The row Enter opens while searching.
    pub fn cursor(&self) -> Option<Arc<str>> {
        self.cursor.clone().filter(|_| self.searching)
    }

    /// Opens the search field with the focus in it, or closes and clears it.
    pub fn toggle_search(&mut self, entries: &[RepoEntry]) -> Task<Message> {
        if self.searching {
            self.close_search();
            return Task::none();
        }
        self.searching = true;
        self.set_query(String::new(), entries);
        iced::widget::operation::focus(SEARCH_ID)
    }

    pub fn close_search(&mut self) {
        self.searching = false;
        self.query.clear();
        self.cursor = None;
    }

    /// Filters the list; the first result becomes the one Enter opens.
    pub fn set_query(&mut self, query: String, entries: &[RepoEntry]) {
        self.query = query;
        self.cursor = self.paths(entries).into_iter().next();
    }

    /// Moves `delta` rows from the search cursor, or from `current` when not searching,
    /// and returns the row reached. Stops at either end of the list.
    pub fn step(
        &mut self,
        delta: i32,
        entries: &[RepoEntry],
        current: Option<&str>,
    ) -> Option<Arc<str>> {
        let paths = self.paths(entries);
        let from = if self.searching {
            self.cursor.as_deref()
        } else {
            current
        };
        let last = paths.len().checked_sub(1)?;
        let index = match from.and_then(|from| paths.iter().position(|path| &**path == from)) {
            Some(index) => index.saturating_add_signed(delta as isize).min(last),
            None if delta < 0 => last,
            None => 0,
        };
        let target = paths[index].clone();
        if self.searching {
            self.cursor = Some(target.clone());
        }
        Some(target)
    }

    /// Every listed row, top to bottom.
    fn paths(&self, entries: &[RepoEntry]) -> Vec<Arc<str>> {
        repo_list::groups(entries, &HashMap::new(), &self.query)
            .into_iter()
            .flat_map(|group| group.rows)
            .map(|row| row.path)
            .collect()
    }

    pub fn view(
        &self,
        entries: &[RepoEntry],
        statuses: &HashMap<String, QuickStatus>,
        current: Option<&str>,
        preferences_open: bool,
    ) -> Element<'_, Message> {
        let typo = Typography::current();
        let groups = repo_list::groups(entries, statuses, &self.query);
        let selected = if self.searching {
            self.cursor.as_deref()
        } else {
            current
        };

        let mut list = Column::new().spacing(2);
        for group in groups {
            let label = match group.label_key {
                "sidebar_results" if group.rows.len() == 1 => t("sidebar_results_one").to_owned(),
                key => tf(key, &[("count", &group.rows.len().to_string())]),
            };
            list = list.push(
                container(
                    text(label)
                        .size(typo.sz(10))
                        .font(typo.regular)
                        .color(Palette::TEXT_DIMMEST()),
                )
                .padding(iced::Padding {
                    top: 10.0,
                    right: 10.0,
                    bottom: 2.0,
                    left: 10.0,
                }),
            );
            for repo in group.rows {
                let is_selected = selected == Some(&*repo.path);
                let menu_open = self.menu.as_deref() == Some(&*repo.path);
                list = list.push(row_view(repo, &self.query, is_selected, menu_open));
            }
        }

        let visible = entries.iter().filter(|entry| !entry.hidden).count();
        let add = button(
            text(t("sidebar_add"))
                .size(typo.sz(13))
                .font(typo.regular)
                .color(Palette::ACCENT()),
        )
        .on_press(Message::AddRepository)
        .width(Length::Fill)
        .padding([8, 10])
        .style(|_, status| button::Style {
            background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                .then(|| Palette::BG_CARD_HOVER().into()),
            border: Border::default().rounded(6),
            ..Default::default()
        });
        let divider =
            container(Space::new().width(Length::Fill).height(1)).style(|_| container::Style {
                background: Some(Palette::DIVIDER().into()),
                ..Default::default()
            });
        let hint = if self.searching {
            t("sidebar_search_hint")
        } else {
            t("sidebar_hint")
        };

        let content = column![
            identity_button(
                &typo,
                "GitSpace", // i18n: not user-visible (program name)
                22,
                preferences_open,
                Message::TogglePreferences
            ),
            Space::new().height(6),
            self.label_row(&typo, visible),
            scrollable(list).height(Length::Fill),
            divider,
            add,
            container(nav_hint(&typo, hint)).padding([4, 10]),
        ]
        .spacing(2)
        .padding([14, 10]);

        container(content)
            .width(240)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Palette::BG_SIDEBAR().into()),
                ..Default::default()
            })
            .into()
    }

    /// "Repositories · N" with the magnifier, or the search field in its place. Both are
    /// the same height, so opening the search moves nothing below.
    fn label_row(&self, typo: &Typography, count: usize) -> Element<'_, Message> {
        let height = typo.sz(30);
        if !self.searching {
            let label = tf("sidebar_repositories", &[("count", &count.to_string())]);
            return container(
                row![
                    nav_label(typo, &label),
                    Space::new().width(Length::Fill),
                    gs_button::icon(
                        SEARCH_GLYPH,
                        Some(Message::ToggleSearch),
                        t("sidebar_search_tip")
                    ),
                ]
                .align_y(Alignment::Center),
            )
            .height(height)
            .padding([0, 8])
            .align_y(Alignment::Center)
            .into();
        }

        let field = text_input(t("sidebar_search_placeholder"), &self.query)
            .id(SEARCH_ID)
            .on_input(Message::Query)
            .on_submit(Message::Activate)
            .size(typo.sz(13))
            .font(typo.regular)
            .padding(0)
            .style(|_, _| text_input::Style {
                background: iced::Color::TRANSPARENT.into(),
                border: Border::default(),
                icon: Palette::TEXT_DIMMER(),
                placeholder: Palette::TEXT_PLACEHOLDER(),
                value: Palette::TEXT_PRIMARY(),
                selection: Palette::ACCENT().scale_alpha(0.35),
            });
        container(
            row![
                text(SEARCH_GLYPH)
                    .size(typo.sz(13))
                    .font(fonts::glyph_font())
                    .color(Palette::TEXT_DIMMER()),
                field,
                gs_button::icon(
                    icons::CLOSE,
                    Some(Message::ToggleSearch),
                    t("sidebar_search_close")
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .height(height)
        .padding([0, 8])
        .align_y(Alignment::Center)
        .style(|_| container::Style {
            background: Some(Palette::BG_INPUT().into()),
            border: Border::default().rounded(6),
            ..Default::default()
        })
        .into()
    }
}

/// What the right end of a row shows.
#[derive(Debug, PartialEq, Eq)]
enum Badge {
    /// A label from the i18n table.
    Label(&'static str),
    /// Changed files, then ↑ ahead and ↓ behind, each only when not zero.
    Counts(String),
    Nothing,
}

fn badge(status: Option<&QuickStatus>) -> Badge {
    let Some(status) = status else {
        return Badge::Nothing;
    };
    if status.missing {
        return Badge::Label("sidebar_missing");
    }
    let parts: Vec<String> = [
        (status.changed > 0).then(|| status.changed.to_string()),
        (status.ahead > 0).then(|| format!("↑{}", status.ahead)),
        (status.behind > 0).then(|| format!("↓{}", status.behind)),
    ]
    .into_iter()
    .flatten()
    .collect();
    if parts.is_empty() {
        Badge::Nothing
    } else {
        Badge::Counts(parts.join(" "))
    }
}

fn row_view<'a>(repo: Row, query: &str, selected: bool, menu_open: bool) -> Element<'a, Message> {
    let typo = Typography::current();
    let missing = repo.status.as_ref().is_some_and(|status| status.missing);
    let on_accent = contrast_on(Palette::ACCENT());
    let name_color = if selected {
        on_accent
    } else if missing {
        Palette::TEXT_DIMMER()
    } else {
        Palette::TEXT_MUTED()
    };

    let mut spans: Vec<Span<'a, (), Font>> = Vec::new();
    let mut at = 0;
    for range in repo_list::match_ranges(&repo.name, query.trim()) {
        spans.push(span(repo.name[at..range.start].to_owned()).color(name_color));
        let matched = span(repo.name[range.clone()].to_owned()).font(typo.bold);
        spans.push(if selected {
            matched.color(on_accent).underline(true)
        } else {
            matched.color(Palette::ACCENT())
        });
        at = range.end;
    }
    spans.push(span(repo.name[at..].to_owned()).color(name_color));

    let badge_color = if selected {
        on_accent.scale_alpha(0.7)
    } else {
        Palette::TEXT_DIMMEST()
    };
    let badge_text = match badge(repo.status.as_ref()) {
        Badge::Label(key) => t(key).to_owned(),
        Badge::Counts(counts) => counts,
        Badge::Nothing => String::new(),
    };

    let content = row![
        rich_text(spans)
            .size(typo.sz(13))
            .font(typo.regular)
            .width(Length::Fill),
        text(badge_text)
            .size(typo.sz(10))
            .font(typo.regular)
            .color(badge_color),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let item = mouse_area(
        button(content)
            .on_press(Message::Select(repo.path.clone()))
            .padding([6, 10])
            .width(Length::Fill)
            .style(selection_style(selected)),
    )
    .on_right_press(Message::ContextMenu(Some(repo.path.clone())));

    let pin_tip = if repo.pinned {
        t("sidebar_unpin")
    } else {
        t("sidebar_pin")
    };
    let pin = container(gs_button::icon(
        PIN_GLYPH,
        Some(Message::Pin(repo.path.clone(), !repo.pinned)),
        pin_tip,
    ))
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Alignment::End)
    .align_y(Alignment::Center);

    let mut entries = vec![menu::Entry::Item(menu::MenuItem {
        label: pin_tip.to_owned(),
        shortcut: None,
        on_select: Message::Pin(repo.path.clone(), !repo.pinned),
        danger: false,
    })];
    // A folder that is gone cannot be opened; the menu then only offers to remove it.
    if !missing {
        for (key, message) in [
            (
                "sidebar_open_folder",
                Message::OpenFolder(repo.path.clone()),
            ),
            (
                "sidebar_open_terminal",
                Message::OpenTerminal(repo.path.clone()),
            ),
        ] {
            entries.push(menu::Entry::Item(menu::MenuItem {
                label: t(key).to_owned(),
                shortcut: None,
                on_select: message,
                danger: false,
            }));
        }
    }
    entries.push(menu::Entry::Separator);
    entries.push(menu::Entry::Item(menu::MenuItem {
        label: t("sidebar_remove").to_owned(),
        shortcut: None,
        on_select: Message::Hide(repo.path),
        danger: false,
    }));

    menu::popup(
        hover(item, pin),
        menu_open,
        entries,
        Message::ContextMenu(None),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_repo_row_is_marked() {
        let missing = QuickStatus {
            missing: true,
            ..QuickStatus::default()
        };
        assert_eq!(badge(Some(&missing)), Badge::Label("sidebar_missing"));
        assert_ne!(t("sidebar_missing"), "sidebar_missing");

        let busy = QuickStatus {
            changed: 4,
            ahead: 2,
            ..QuickStatus::default()
        };
        assert_eq!(badge(Some(&busy)), Badge::Counts("4 ↑2".into()));
        assert_eq!(badge(Some(&QuickStatus::default())), Badge::Nothing);
        assert_eq!(badge(None), Badge::Nothing);
    }

    fn entry(path: &str, last_opened: i64) -> RepoEntry {
        RepoEntry {
            path: path.to_string(),
            last_opened: Some(last_opened),
            ..RepoEntry::default()
        }
    }

    #[test]
    fn step_moves_from_the_current_row_and_stops_at_the_ends() {
        let entries = [entry("/a", 3), entry("/b", 2), entry("/c", 1)];
        let mut sidebar = Sidebar::default();
        assert_eq!(sidebar.step(1, &entries, Some("/a")).as_deref(), Some("/b"));
        assert_eq!(sidebar.step(1, &entries, Some("/c")).as_deref(), Some("/c"));
        assert_eq!(sidebar.step(-1, &entries, None).as_deref(), Some("/c"));

        let _ = sidebar.toggle_search(&entries);
        assert_eq!(sidebar.cursor().as_deref(), Some("/a"));
        sidebar.step(1, &entries, Some("/a"));
        assert_eq!(sidebar.cursor().as_deref(), Some("/b"));
        sidebar.close_search();
        assert_eq!(sidebar.cursor(), None);
    }
}
