//! Preferences → Git: cloning, background fetch, pinned branches and logs.

use colony_ui::theme::Palette;
use colony_ui::widgets::{category_heading, dropdown, functional_toggle, setting_row};
use colony_ui::Typography;
use iced::widget::{container, row, space, text, text_input, Column};
use iced::{Alignment, Length};

use super::{Message, PreferencesPage, FETCH_INTERVALS};
use crate::config::{
    LoggingOptions, Preferences, MAX_LOG_RETENTION_FILES, MIN_LOG_RETENTION_FILES,
};
use crate::i18n::{t, tf};
use crate::ui::changes::field;
use crate::ui::widgets::button as gs_button;

pub(super) fn view<'a>(
    page: &'a PreferencesPage,
    typo: &Typography,
    prefs: &'a Preferences,
    logging: &LoggingOptions,
) -> Column<'a, Message> {
    let folder = page
        .clone_path
        .as_deref()
        .unwrap_or(prefs.default_clone_path());
    let cloning = Column::new()
        .push(setting_row(
            typo,
            t("prefs_default_folder"),
            t("prefs_default_folder_desc"),
            row![
                container(field(
                    text_input("", folder)
                        .on_input(Message::ClonePathChanged)
                        .on_submit(Message::ClonePathSubmitted),
                    None,
                ))
                .width(280),
                gs_button::secondary(t("prefs_browse"), Some(Message::BrowseClonePath)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ))
        .push(functional_toggle(
            typo,
            t("prefs_scan_folder"),
            t("prefs_scan_folder_desc"),
            prefs.scan_clone_folder(),
            Message::ScanCloneFolder(!prefs.scan_clone_folder()),
        ));

    let intervals = FETCH_INTERVALS.map(|minutes| {
        (
            minutes,
            tf("prefs_minutes", &[("count", &minutes.to_string())]),
        )
    });
    let fetch = Column::new()
        .push(functional_toggle(
            typo,
            t("prefs_fetch"),
            t("prefs_fetch_toggle_desc"),
            prefs.auto_fetch_enabled(),
            Message::AutoFetch(!prefs.auto_fetch_enabled()),
        ))
        .push(setting_row(
            typo,
            t("prefs_fetch_interval"),
            t("prefs_fetch_interval_desc"),
            dropdown(
                typo,
                intervals,
                &prefs.auto_fetch_interval_minutes(),
                Message::FetchInterval,
            ),
        ));

    let mut pinned = Column::new().spacing(4);
    if prefs.pinned_branches().is_empty() {
        pinned = pinned.push(
            text(t("prefs_pinned_none"))
                .size(typo.sz(12))
                .font(typo.regular)
                .color(Palette::TEXT_MUTED()),
        );
    }
    for branch in prefs.pinned_branches() {
        pinned = pinned.push(
            row![
                text(branch.clone())
                    .size(typo.sz(13))
                    .font(typo.regular)
                    .color(Palette::TEXT_PRIMARY()),
                space().width(Length::Fill),
                gs_button::ghost(t("prefs_unpin"), Some(Message::Unpin(branch.clone()))),
            ]
            .padding([2, 4])
            .align_y(Alignment::Center),
        );
    }

    let kept =
        (MIN_LOG_RETENTION_FILES..=MAX_LOG_RETENTION_FILES).map(|files| (files, files.to_string()));
    let logs = Column::new()
        .spacing(8)
        .push(setting_row(
            typo,
            t("prefs_logs_kept"),
            t("prefs_logs_kept_desc"),
            dropdown(
                typo,
                kept,
                &logging.retention_files(),
                Message::LogRetention,
            ),
        ))
        .push(gs_button::secondary(
            t("prefs_open_logs"),
            Some(Message::OpenFolder(page.logs_dir.clone())),
        ));

    category_heading(typo, t("prefs_cat_git"), t("prefs_git_desc"))
        .push(page.section(
            typo,
            "cloning",
            t("prefs_section_cloning"),
            t("prefs_cloning_desc"),
            cloning,
        ))
        .push(page.section(
            typo,
            "fetch",
            t("prefs_section_fetch"),
            t("prefs_fetch_desc"),
            fetch,
        ))
        .push(page.section(
            typo,
            "pinned",
            t("prefs_section_pinned"),
            t("prefs_pinned_desc"),
            pinned,
        ))
        .push(page.section(
            typo,
            "logs",
            t("prefs_section_logs"),
            t("prefs_logs_desc"),
            logs,
        ))
}
