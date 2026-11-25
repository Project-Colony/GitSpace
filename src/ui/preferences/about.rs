//! Preferences → About: version, licence, source code and where GitSpace keeps its files.

use std::path::Path;

use colony_ui::theme::Palette;
use colony_ui::widgets::{category_heading, setting_row};
use colony_ui::Typography;
use iced::widget::{row, text, Column};
use iced::Element;

use super::{Message, PreferencesPage};
use crate::i18n::t;
use crate::ui::widgets::button as gs_button;

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

pub(super) fn view<'a>(page: &'a PreferencesPage, typo: &Typography) -> Column<'a, Message> {
    let value = |value: &str| -> Element<'a, Message> {
        text(value.to_string())
            .size(typo.sz(12))
            .font(typo.regular)
            .color(Palette::TEXT_SECONDARY())
            .into()
    };
    let folder = |title: &str, path: &Path| {
        let shown = path.display().to_string();
        setting_row(
            typo,
            title,
            &shown,
            row![
                gs_button::secondary(t("prefs_copy"), Some(Message::Copy(shown.clone()))),
                gs_button::secondary(
                    t("prefs_open"),
                    Some(Message::OpenFolder(path.to_path_buf()))
                ),
            ]
            .spacing(8),
        )
    };
    category_heading(
        typo,
        colony_ui::i18n::t(super::ABOUT_CATEGORY),
        t("prefs_about_desc"),
    )
    .push(setting_row(
        typo,
        t("prefs_version"),
        "",
        value(env!("CARGO_PKG_VERSION")),
    ))
    .push(setting_row(
        typo,
        t("prefs_licence"),
        "",
        value(env!("CARGO_PKG_LICENSE")),
    ))
    .push(setting_row(
        typo,
        t("prefs_source_code"),
        REPOSITORY,
        gs_button::secondary(t("prefs_open"), Some(Message::OpenUrl(REPOSITORY))),
    ))
    .push(folder(t("prefs_config_folder"), &page.config_dir))
    .push(folder(t("prefs_logs_folder"), &page.logs_dir))
}
