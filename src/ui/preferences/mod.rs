//! The Preferences page, built on the colony-ui kit: General, Appearance and Accessibility
//! from the kit, then GitSpace's Accounts, Git and Network, then About.
//!
//! There is no Save button: every message changes the preferences the app hands in, and
//! the app applies and saves them as soon as `update` returns.

mod about;
pub mod accounts;
mod git;
mod network;

use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;

use colony_ui::preferences::Change;
use colony_ui::theme::Palette;
use colony_ui::widgets::{
    category_heading, category_list, collapsible_section, dropdown, functional_toggle, nav_label,
    preferences_page, section_description, setting_row, PreferencesView, ABOUT_CATEGORY,
    STANDARD_CATEGORIES,
};
use colony_ui::Typography;
use iced::widget::{column, container, row, text, Column};
use iced::{Alignment, Element, Padding, Task};

use crate::auth::AuthManager;
use crate::config::{Language, LoggingOptions, Preferences};
use crate::i18n::{t, tf};
use crate::ui::repo_header::open_folder;
use crate::ui::widgets::button as gs_button;
use crate::ui::widgets::toast::Toast;

/// Category keys, in the order the list shows them.
pub const CATEGORIES: [&str; 7] = [
    STANDARD_CATEGORIES[0],
    STANDARD_CATEGORIES[1],
    STANDARD_CATEGORIES[2],
    "prefs_cat_accounts",
    "prefs_cat_git",
    "prefs_cat_network",
    ABOUT_CATEGORY,
];
const ACCOUNTS: usize = 3;
/// GitSpace's own categories, listed under the "GitSpace" separator.
const GITSPACE: Range<usize> = 3..6;

/// Background fetch intervals offered, in minutes.
pub const FETCH_INTERVALS: [u64; 5] = [1, 5, 15, 30, 60];

#[derive(Debug, Clone)]
pub enum Message {
    /// Opens or closes the page; a clone folder typed without Enter is applied first.
    Toggle,
    Category(usize),
    /// Opens or closes a collapsible section, by key.
    Section(&'static str),
    Standard(Change),
    Language(Language),
    ReopenLastRepo(bool),
    ImportSettings,
    /// `None` when the file dialog was cancelled.
    ImportResult(Result<Option<Preferences>, String>),
    ExportSettings,
    /// The path written, or `None` when the file dialog was cancelled.
    ExportResult(Result<Option<String>, String>),
    ResetDefaults,
    ConfirmReset,
    CancelReset,

    Accounts(accounts::Message),
    EncryptedTokens(bool),

    ClonePathChanged(String),
    ClonePathSubmitted,
    BrowseClonePath,
    BrowseClonePathResult(Option<String>),
    ScanCloneFolder(bool),
    AutoFetch(bool),
    FetchInterval(u64),
    Unpin(String),
    LogRetention(usize),

    HttpProxy(String),
    HttpsProxy(String),
    IdleTimeout(String),
    PreferHttps(bool),
    AllowSsh(bool),

    OpenFolder(PathBuf),
    Copy(String),
    OpenUrl(&'static str),
}

pub struct PreferencesPage {
    pub category: usize,
    /// Keys of the open sections. All closed each time the page opens.
    pub expanded: HashSet<String>,
    open: bool,
    pub accounts: accounts::Accounts,
    /// A typed clone folder not applied yet: it applies on Enter or when the page closes,
    /// so a half-typed path never starts a folder scan.
    clone_path: Option<String>,
    /// The idle timeout as typed; only a valid number reaches the preferences.
    idle_timeout: Option<String>,
    confirm_reset: bool,
    config_dir: PathBuf,
    logs_dir: PathBuf,
}

impl PreferencesPage {
    pub fn new(auth: AuthManager) -> Self {
        let (config_dir, data_dir) = crate::paths::effective_dirs();
        Self {
            category: 0,
            expanded: HashSet::new(),
            open: false,
            accounts: accounts::Accounts::new(auth),
            clone_path: None,
            idle_timeout: None,
            confirm_reset: false,
            config_dir,
            logs_dir: data_dir.join("logs"),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens or closes the page: the identity button, Close, Ctrl+, and Escape.
    fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.expanded.clear();
            self.clone_path = None;
            self.idle_timeout = None;
            self.confirm_reset = false;
        }
    }

    /// Saves the clone folder typed but not applied yet.
    fn apply_clone_path(&mut self, prefs: &mut Preferences) {
        if let Some(path) = self.clone_path.take() {
            prefs.set_default_clone_path(path);
        }
    }

    /// Opens the page on Accounts.
    pub fn open_accounts(&mut self) {
        if !self.open {
            self.toggle();
        }
        self.category = ACCOUNTS;
    }

    /// Changes `prefs` and `logging` in place; the app saves whatever changed.
    pub fn update(
        &mut self,
        message: Message,
        prefs: &mut Preferences,
        logging: &mut LoggingOptions,
    ) -> (Task<Message>, Option<Toast>) {
        let mut task = Task::none();
        let mut toast = None;
        match message {
            Message::Toggle => {
                if self.open {
                    self.apply_clone_path(prefs);
                }
                self.toggle();
            }
            Message::Category(index) => self.category = index.min(CATEGORIES.len() - 1),
            Message::Section(key) => {
                if !self.expanded.remove(key) {
                    self.expanded.insert(key.to_string());
                }
            }
            // `update` also applies the change.
            Message::Standard(change) => prefs.standard.update(change),
            Message::Language(language) => set_language(prefs, language),
            Message::ReopenLastRepo(on) => prefs.set_reopen_last_repo(on),
            Message::ImportSettings => {
                task = Task::perform(
                    tokio::task::spawn_blocking(|| {
                        rfd::FileDialog::new()
                            .add_filter("JSON", &["json"])
                            .pick_file()
                            .map(|path| Preferences::from_path(&path).map_err(|e| e.to_string()))
                            .transpose()
                    }),
                    |result| Message::ImportResult(result.unwrap_or_else(|e| Err(e.to_string()))),
                )
            }
            Message::ImportResult(Ok(Some(imported))) => {
                *prefs = imported;
                self.clone_path = None;
                self.idle_timeout = None;
                toast = Some(Toast::success(t("prefs_imported")));
            }
            Message::ImportResult(Ok(None)) | Message::ExportResult(Ok(None)) => {}
            Message::ImportResult(Err(err)) => {
                toast = Some(Toast::error(t("prefs_import_failed"), err));
            }
            Message::ExportSettings => {
                let current = prefs.clone();
                task = Task::perform(
                    tokio::task::spawn_blocking(move || {
                        rfd::FileDialog::new()
                            .add_filter("JSON", &["json"])
                            .set_file_name("gitspace-preferences.json")
                            .save_file()
                            .map(|path| {
                                current
                                    .save_to_path(&path)
                                    .map(|_| path.display().to_string())
                                    .map_err(|e| e.to_string())
                            })
                            .transpose()
                    }),
                    |result| Message::ExportResult(result.unwrap_or_else(|e| Err(e.to_string()))),
                )
            }
            Message::ExportResult(Ok(Some(path))) => {
                toast = Some(Toast::success(tf("prefs_exported", &[("path", &path)])));
            }
            Message::ExportResult(Err(err)) => {
                toast = Some(Toast::error(t("prefs_export_failed"), err));
            }
            Message::ResetDefaults => self.confirm_reset = true,
            Message::CancelReset => self.confirm_reset = false,
            Message::ConfirmReset => {
                *prefs = Preferences::default();
                *logging = LoggingOptions::default();
                self.confirm_reset = false;
                self.clone_path = None;
                self.idle_timeout = None;
            }

            Message::Accounts(msg) => task = self.accounts.update(msg).map(Message::Accounts),
            Message::EncryptedTokens(on) => prefs.set_allow_encrypted_tokens(on),

            Message::ClonePathChanged(path) => self.clone_path = Some(path),
            Message::ClonePathSubmitted => self.apply_clone_path(prefs),
            Message::BrowseClonePath => {
                task = Task::perform(
                    tokio::task::spawn_blocking(|| {
                        rfd::FileDialog::new()
                            .pick_folder()
                            .map(|path| path.display().to_string())
                    }),
                    |result| Message::BrowseClonePathResult(result.ok().flatten()),
                )
            }
            Message::BrowseClonePathResult(Some(path)) => {
                self.clone_path = None;
                prefs.set_default_clone_path(path);
            }
            Message::BrowseClonePathResult(None) => {}
            Message::ScanCloneFolder(on) => prefs.set_scan_clone_folder(on),
            Message::AutoFetch(on) => prefs.set_auto_fetch_enabled(on),
            Message::FetchInterval(minutes) => prefs.set_auto_fetch_interval_minutes(minutes),
            Message::Unpin(branch) => {
                let kept = prefs
                    .pinned_branches()
                    .iter()
                    .filter(|name| **name != branch)
                    .cloned()
                    .collect();
                prefs.set_pinned_branches(kept);
            }
            Message::LogRetention(files) => logging.set_retention_files(files),

            Message::HttpProxy(proxy) => prefs.network_mut().http_proxy = proxy,
            Message::HttpsProxy(proxy) => prefs.network_mut().https_proxy = proxy,
            Message::IdleTimeout(typed) => {
                if let Some(secs) = typed.trim().parse::<u64>().ok().filter(|secs| *secs > 0) {
                    prefs.network_mut().network_timeout_secs = secs;
                }
                self.idle_timeout = Some(typed);
            }
            Message::PreferHttps(on) => prefs.network_mut().use_https = on,
            Message::AllowSsh(on) => prefs.network_mut().allow_ssh = on,

            Message::OpenFolder(path) => {
                let path = path.display().to_string();
                if let Err(err) = open_folder(&path) {
                    toast = Some(Toast::error(
                        t("open_folder_failed"),
                        format!("{path}: {err}"),
                    ));
                }
            }
            Message::Copy(value) => task = iced::clipboard::write(value),
            Message::OpenUrl(url) => {
                if let Err(err) = webbrowser::open(url) {
                    toast = Some(Toast::error(
                        t("prefs_open_link_failed"),
                        format!("{url}: {err}"),
                    ));
                }
            }
        }
        (task, toast)
    }

    // ─── View ─────────────────────────────────────────────────────────

    /// The whole page. `on_close` is the message the identity button sends.
    pub fn view<'a, M: Clone + 'a>(
        &'a self,
        prefs: &'a Preferences,
        logging: &'a LoggingOptions,
        wrap: fn(Message) -> M,
        on_close: M,
    ) -> Element<'a, M> {
        let typo = Typography::current();
        let kit = PreferencesView::new(
            typo,
            &prefs.standard,
            &self.expanded,
            Message::Standard,
            Message::Section,
        );
        let content: Column<'a, Message> = match self.category {
            0 => self.general(&typo, kit.general(), prefs),
            1 => kit.appearance(),
            2 => kit.accessibility(),
            3 => self.accounts_view(&typo, prefs),
            4 => git::view(self, &typo, prefs, logging),
            5 => network::view(self, &typo, prefs),
            _ => about::view(self, &typo),
        };
        preferences_page(
            &typo,
            Element::from(self.categories(&typo)).map(wrap),
            Element::from(content).map(wrap),
            on_close,
        )
    }

    /// The kit's category lists, with GitSpace's own under a separator and About apart.
    fn categories<'a>(&self, typo: &Typography) -> Column<'a, Message> {
        let list = |range: Range<usize>| {
            let start = range.start;
            category_list(
                typo,
                CATEGORIES[range].iter().map(|key| category_label(key)),
                self.category.wrapping_sub(start),
                move |index| Message::Category(start + index),
            )
        };
        let gap = |top, label: &str| {
            container(nav_label(typo, label)).padding(Padding {
                top,
                right: 0.0,
                bottom: 4.0,
                left: 10.0,
            })
        };
        column![
            list(0..GITSPACE.start),
            gap(12.0, t("prefs_cat_gitspace")),
            list(GITSPACE),
            gap(8.0, ""),
            list(GITSPACE.end..CATEGORIES.len()),
        ]
    }

    /// A collapsible section: its description, then `body`.
    fn section<'a>(
        &self,
        typo: &Typography,
        key: &'static str,
        title: &str,
        description: &str,
        body: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        collapsible_section(
            typo,
            title,
            self.expanded.contains(key),
            Message::Section(key),
            column![section_description(typo, description), body.into()].into(),
        )
    }

    /// The kit's General heading, then Language, Startup and the preferences file.
    fn general<'a>(
        &self,
        typo: &Typography,
        heading: Column<'a, Message>,
        prefs: &Preferences,
    ) -> Column<'a, Message> {
        let languages = [
            (Language::System, t("prefs_language_system").to_string()),
            (Language::English, t("prefs_language_english").to_string()),
            (Language::French, t("prefs_language_french").to_string()),
        ];
        let language = setting_row(
            typo,
            t("prefs_language"),
            t("prefs_language_row_desc"),
            dropdown(typo, languages, &prefs.language(), Message::Language),
        );
        let reopen = functional_toggle(
            typo,
            t("prefs_reopen_last"),
            t("prefs_reopen_last_desc"),
            prefs.reopen_last_repo(),
            Message::ReopenLastRepo(!prefs.reopen_last_repo()),
        );
        let reset: Element<'a, Message> = if self.confirm_reset {
            row![
                text(t("prefs_reset_confirm"))
                    .size(typo.sz(12))
                    .font(typo.regular)
                    .color(Palette::TEXT_PRIMARY()),
                gs_button::danger(t("prefs_reset_yes"), Some(Message::ConfirmReset)),
                gs_button::secondary(t("prefs_cancel"), Some(Message::CancelReset)),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
        } else {
            gs_button::danger(t("prefs_reset"), Some(Message::ResetDefaults))
        };
        let file = column![
            row![
                gs_button::secondary(t("prefs_import"), Some(Message::ImportSettings)),
                gs_button::secondary(t("prefs_export"), Some(Message::ExportSettings)),
            ]
            .spacing(10),
            reset,
        ]
        .spacing(12);

        heading
            .push(self.section(
                typo,
                "language",
                t("prefs_section_language"),
                t("prefs_language_desc"),
                language,
            ))
            .push(self.section(
                typo,
                "startup",
                t("prefs_section_startup"),
                t("prefs_startup_desc"),
                reopen,
            ))
            .push(self.section(
                typo,
                "file",
                t("prefs_section_file"),
                t("prefs_file_desc"),
                file,
            ))
    }

    /// Accounts: the saved accounts as cards, then the sign-in and token sections.
    fn accounts_view<'a>(&'a self, typo: &Typography, prefs: &Preferences) -> Column<'a, Message> {
        let accounts = &self.accounts;
        let storage = functional_toggle(
            typo,
            t("prefs_encrypted_fallback"),
            t("prefs_encrypted_fallback_desc"),
            prefs.allow_encrypted_tokens(),
            Message::EncryptedTokens(!prefs.allow_encrypted_tokens()),
        );
        category_heading(typo, t("prefs_cat_accounts"), t("prefs_accounts_desc"))
            .push(accounts.cards().map(Message::Accounts))
            .push(self.section(
                typo,
                "github",
                t("prefs_section_github"),
                t("prefs_github_desc"),
                accounts.github().map(Message::Accounts),
            ))
            .push(self.section(
                typo,
                "gitlab",
                t("prefs_section_gitlab"),
                t("prefs_gitlab_desc"),
                accounts.gitlab().map(Message::Accounts),
            ))
            .push(self.section(
                typo,
                "token",
                t("prefs_section_token"),
                t("prefs_token_desc"),
                accounts.token().map(Message::Accounts),
            ))
            .push(self.section(
                typo,
                "storage",
                t("prefs_section_storage"),
                t("prefs_storage_desc"),
                storage,
            ))
            .push(self.section(
                typo,
                "client_ids",
                t("prefs_section_client_ids"),
                t("prefs_client_ids_desc"),
                accounts.client_ids().map(Message::Accounts),
            ))
    }
}

/// The kit translates its own category names; GitSpace's come from its table.
fn category_label(key: &str) -> &'static str {
    if key.starts_with("settings_") {
        colony_ui::i18n::t(key)
    } else {
        t(key)
    }
}

/// Stores the language and switches both label tables to it at once.
fn set_language(prefs: &mut Preferences, language: Language) {
    prefs.set_language(language);
    colony_ui::i18n::set_locale(language.locale());
}

#[cfg(test)]
mod tests {
    use super::*;
    use colony_ui::i18n::Locale;

    #[test]
    fn categories_are_in_colony_order() {
        let [general, appearance, accessibility] = STANDARD_CATEGORIES;
        assert_eq!(
            CATEGORIES.to_vec(),
            [
                general,
                appearance,
                accessibility,
                "prefs_cat_accounts",
                "prefs_cat_git",
                "prefs_cat_network",
                ABOUT_CATEGORY,
            ]
        );
        assert_eq!(CATEGORIES[ACCOUNTS], "prefs_cat_accounts");
        // Every label is translated, by the kit or by GitSpace's table.
        for key in CATEGORIES.iter().chain(["prefs_cat_gitspace"].iter()) {
            assert_ne!(category_label(key), *key);
        }
    }

    #[test]
    fn closing_the_page_applies_a_typed_clone_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut page = PreferencesPage::new(AuthManager::in_dir(dir.path()));
        let mut prefs = Preferences::default();
        let mut logging = LoggingOptions::default();
        for message in [
            Message::Toggle,
            Message::ClonePathChanged("/tmp/gitspace-clones".to_string()),
            Message::Toggle,
        ] {
            let _ = page.update(message, &mut prefs, &mut logging);
        }
        assert!(!page.is_open());
        assert_eq!(prefs.default_clone_path(), "/tmp/gitspace-clones");
    }

    #[test]
    fn interval_choices_are_the_spec_values() {
        assert_eq!(FETCH_INTERVALS.to_vec(), [1, 5, 15, 30, 60]);
    }

    #[test]
    fn language_change_sets_the_locale() {
        let mut prefs = Preferences::default();
        set_language(&mut prefs, Language::French);
        // Read and put back at once: the locale is shared with tests running alongside.
        let seen = colony_ui::i18n::locale();
        set_language(&mut prefs, Language::English);
        assert_eq!(seen, Locale::Fr);
        assert_eq!(colony_ui::i18n::locale(), Locale::En);
        assert_eq!(prefs.language(), Language::English);
    }
}
