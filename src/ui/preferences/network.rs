//! Preferences → Network: proxies, the idle timeout and the allowed protocols.

use colony_ui::widgets::{category_heading, functional_toggle, setting_row};
use colony_ui::Typography;
use iced::widget::{container, text_input, Column};

use super::{Message, PreferencesPage};
use crate::config::Preferences;
use crate::i18n::t;
use crate::ui::changes::field;

pub(super) fn view<'a>(
    page: &'a PreferencesPage,
    typo: &Typography,
    prefs: &'a Preferences,
) -> Column<'a, Message> {
    let network = prefs.network();
    let input = |placeholder: &str, value: &str, on_input: fn(String) -> Message, width: f32| {
        container(field(
            text_input(placeholder, value).on_input(on_input),
            None,
        ))
        .width(width)
    };
    let proxy = Column::new()
        .push(setting_row(
            typo,
            t("prefs_http_proxy"),
            t("prefs_http_proxy_desc"),
            input(
                t("prefs_proxy_example"),
                &network.http_proxy,
                Message::HttpProxy,
                260.0,
            ),
        ))
        .push(setting_row(
            typo,
            t("prefs_https_proxy"),
            t("prefs_https_proxy_desc"),
            input(
                t("prefs_proxy_example"),
                &network.https_proxy,
                Message::HttpsProxy,
                260.0,
            ),
        ));

    let timeout = page
        .idle_timeout
        .clone()
        .unwrap_or_else(|| network.network_timeout_secs.to_string());
    let connection = Column::new()
        .push(setting_row(
            typo,
            t("prefs_idle_timeout"),
            t("prefs_idle_timeout_desc"),
            input("", &timeout, Message::IdleTimeout, 80.0),
        ))
        .push(functional_toggle(
            typo,
            t("prefs_prefer_https"),
            t("prefs_prefer_https_desc"),
            network.use_https,
            Message::PreferHttps(!network.use_https),
        ))
        .push(functional_toggle(
            typo,
            t("prefs_allow_ssh"),
            t("prefs_allow_ssh_desc"),
            network.allow_ssh,
            Message::AllowSsh(!network.allow_ssh),
        ));

    category_heading(typo, t("prefs_cat_network"), t("prefs_network_desc"))
        .push(page.section(
            typo,
            "proxy",
            t("prefs_section_proxy"),
            t("prefs_proxy_desc"),
            proxy,
        ))
        .push(page.section(
            typo,
            "connection",
            t("prefs_section_connection"),
            t("prefs_connection_desc"),
            connection,
        ))
}
