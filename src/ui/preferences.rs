//! Panneau de preferences plein ecran avec sidebar et categories.
//!
//! Remplace le contenu principal quand ouvert. Utilise une sidebar
//! gauche avec 4 categories et un contenu scrollable a droite.

use iced::widget::{
    button, checkbox, column, container, pick_list, row, rule, scrollable, slider, text,
    text_input, Space,
};
use iced::{Alignment, Color, Element, Length, Task};

use crate::config::{
    LoggingOptions, Preferences, ThemeMode, MAX_LOG_RETENTION_FILES, MIN_LOG_RETENTION_FILES,
};
use crate::ui::theme::{with_alpha, Theme};

/// Modes de theme disponibles pour le pick_list.
const THEME_MODES: &[ThemeMode] = &[
    ThemeMode::Latte,
    ThemeMode::Frappe,
    ThemeMode::Macchiato,
    ThemeMode::Mocha,
];

// ─── Categories ───────────────────────────────────────────────────────────

/// Categorie de la sidebar preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferencesCategory {
    General,
    Appearance,
    GitSpace,
}

impl PreferencesCategory {
    pub const ALL: [Self; 3] = [Self::General, Self::Appearance, Self::GitSpace];

    /// Libelle affiche dans la sidebar.
    pub fn label(&self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Apparences",
            Self::GitSpace => "GitSpace",
        }
    }

    /// Icone affichee devant le libelle.
    pub fn icon(&self) -> &'static str {
        match self {
            Self::General => "\u{2699}",     // ⚙
            Self::Appearance => "\u{1f3a8}", // 🎨
            Self::GitSpace => "\u{1f680}",   // 🚀
        }
    }
}

// ─── Messages ─────────────────────────────────────────────────────────────

/// Messages emis par le panneau de preferences.
#[derive(Debug, Clone)]
pub enum Message {
    /// Changer la categorie active dans la sidebar.
    SelectCategory(PreferencesCategory),
    /// Fermer le panneau de preferences.
    Close,

    // -- General --
    /// Chemin de destination par defaut modifie.
    DefaultClonePathChanged(String),
    /// Ouvrir le selecteur de dossier.
    BrowseClonePath,
    /// Resultat du selecteur de dossier.
    BrowseClonePathResult(Option<String>),
    /// Proxy HTTP modifie.
    HttpProxyChanged(String),
    /// Proxy HTTPS modifie.
    HttpsProxyChanged(String),
    /// Timeout reseau modifie.
    NetworkTimeoutChanged(String),
    /// Preference HTTPS modifiee.
    UseHttpsChanged(bool),
    /// Permission SSH modifiee.
    AllowSshChanged(bool),
    /// Recuperation automatique activee/desactivee.
    AutoFetchEnabledChanged(bool),
    /// Intervalle de recuperation automatique modifie.
    AutoFetchIntervalChanged(String),

    // -- Apparences --
    /// Mode de theme modifie.
    ThemeModeChanged(ThemeMode),

    // -- GitSpace --
    /// Nombre de fichiers de log conserves modifie.
    RetentionFilesChanged(f32),
    /// Stockage chiffre autorise/desautorise.
    AllowEncryptedTokensChanged(bool),
    /// Importer les parametres.
    ImportSettings,
    /// Resultat de l'import (`None` si l'utilisateur a annule).
    ImportResult(Result<Option<Preferences>, String>),
    /// Exporter les parametres.
    ExportSettings,
    /// Resultat de l'export : chemin ecrit, ou `None` si l'utilisateur a annule.
    ExportResult(Result<Option<String>, String>),

    // -- Actions globales --
    /// Sauvegarder les preferences.
    SavePreferences,
    /// Demander la reinitialisation des preferences par defaut.
    ResetDefaults,
    /// Confirmer la reinitialisation.
    ConfirmReset,
    /// Annuler la reinitialisation.
    CancelReset,
}

// ─── Etat ─────────────────────────────────────────────────────────────────

/// Panneau de preferences plein ecran.
pub struct PreferencesPanel {
    /// Preferences courantes en cours d'edition.
    preferences: Preferences,
    /// Options de journalisation courantes.
    logging: LoggingOptions,
    /// Dernieres preferences appliquees, restaurees a la fermeture sans sauvegarde.
    applied: Preferences,
    /// Derniere journalisation appliquee.
    applied_logging: LoggingOptions,
    /// Preferences en attente d'application par l'app.
    pending_preferences: Option<Preferences>,
    /// Journalisation en attente d'application.
    pending_logging: Option<LoggingOptions>,
    /// Statut import/export.
    import_status: Option<String>,
    export_status: Option<String>,
    /// Erreur d'import ou d'export.
    io_error: Option<String>,
    /// La reinitialisation attend une confirmation.
    confirm_reset: bool,
    /// Le panneau est-il ouvert ?
    open: bool,
    /// Categorie active de la sidebar.
    active_category: PreferencesCategory,
    /// Tampon texte pour le timeout reseau.
    timeout_buffer: String,
    /// Tampon texte pour l'intervalle de fetch.
    fetch_interval_buffer: String,
}

impl PreferencesPanel {
    /// Cree un nouveau panneau de preferences.
    pub fn new(preferences: Preferences, logging: LoggingOptions) -> Self {
        let timeout_buffer = preferences.network().network_timeout_secs.to_string();
        let fetch_interval_buffer = preferences.auto_fetch_interval_minutes().to_string();
        Self {
            applied: preferences.clone(),
            applied_logging: logging,
            preferences,
            logging,
            pending_preferences: None,
            pending_logging: None,
            import_status: None,
            export_status: None,
            io_error: None,
            confirm_reset: false,
            open: false,
            active_category: PreferencesCategory::General,
            timeout_buffer,
            fetch_interval_buffer,
        }
    }

    /// Enregistre les preferences appliquees par l'application. Les modifications en cours
    /// restent a l'ecran tant que le panneau est ouvert.
    pub fn set_preferences(&mut self, preferences: Preferences) {
        self.applied = preferences;
        if !self.open {
            self.discard_edits();
        }
    }

    /// Remplace la copie editee par les dernieres valeurs appliquees.
    fn discard_edits(&mut self) {
        self.preferences = self.applied.clone();
        self.logging = self.applied_logging;
        self.timeout_buffer = self.preferences.network().network_timeout_secs.to_string();
        self.fetch_interval_buffer = self.preferences.auto_fetch_interval_minutes().to_string();
        self.confirm_reset = false;
    }

    /// Transmet `preferences` a l'application et les retient comme appliquees.
    fn apply(&mut self, preferences: Preferences) {
        self.applied = preferences.clone();
        self.pending_preferences = Some(preferences);
    }

    /// Le panneau est-il ouvert ?
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Bascule l'ouverture du panneau. Les modifications non sauvegardees sont abandonnees.
    pub fn toggle(&mut self) {
        self.discard_edits();
        self.open = !self.open;
    }

    /// Recupere les preferences en attente d'application.
    pub fn take_changes(&mut self) -> Option<Preferences> {
        self.pending_preferences.take()
    }

    /// Recupere les changements de journalisation en attente.
    pub fn take_logging_changes(&mut self) -> Option<LoggingOptions> {
        self.pending_logging.take()
    }

    // ─── Update ───────────────────────────────────────────────────────

    /// Traite un message et renvoie une tache Iced.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SelectCategory(cat) => {
                self.active_category = cat;
                Task::none()
            }
            Message::Close => {
                self.discard_edits();
                self.open = false;
                Task::none()
            }

            // -- General --
            Message::DefaultClonePathChanged(path) => {
                self.preferences.set_default_clone_path(path);
                Task::none()
            }
            Message::BrowseClonePath => Task::perform(
                tokio::task::spawn_blocking(|| {
                    rfd::FileDialog::new()
                        .pick_folder()
                        .map(|p| p.display().to_string())
                }),
                |result| Message::BrowseClonePathResult(result.ok().flatten()),
            ),
            Message::BrowseClonePathResult(Some(path)) => {
                self.preferences.set_default_clone_path(path);
                Task::none()
            }
            Message::BrowseClonePathResult(None) => Task::none(),
            Message::HttpProxyChanged(val) => {
                self.preferences.network_mut().http_proxy = val;
                Task::none()
            }
            Message::HttpsProxyChanged(val) => {
                self.preferences.network_mut().https_proxy = val;
                Task::none()
            }
            Message::NetworkTimeoutChanged(val) => {
                self.timeout_buffer = val.clone();
                if let Ok(secs) = val.trim().parse::<u64>() {
                    self.preferences.network_mut().network_timeout_secs = secs;
                }
                Task::none()
            }
            Message::UseHttpsChanged(val) => {
                self.preferences.network_mut().use_https = val;
                Task::none()
            }
            Message::AllowSshChanged(val) => {
                self.preferences.network_mut().allow_ssh = val;
                Task::none()
            }
            Message::AutoFetchEnabledChanged(val) => {
                self.preferences.set_auto_fetch_enabled(val);
                Task::none()
            }
            Message::AutoFetchIntervalChanged(val) => {
                self.fetch_interval_buffer = val.clone();
                if let Ok(minutes) = val.trim().parse::<u64>() {
                    if minutes > 0 {
                        self.preferences.set_auto_fetch_interval_minutes(minutes);
                    }
                }
                Task::none()
            }

            // -- Apparences --
            Message::ThemeModeChanged(mode) => {
                // Le theme s'applique tout de suite, sans les autres modifications en cours.
                self.preferences.set_theme_mode(mode);
                let mut applied = self.applied.clone();
                applied.set_theme_mode(mode);
                self.apply(applied);
                Task::none()
            }

            // -- GitSpace --
            Message::RetentionFilesChanged(val) => {
                self.logging.set_retention_files(val as usize);
                Task::none()
            }
            Message::AllowEncryptedTokensChanged(val) => {
                self.preferences.set_allow_encrypted_tokens(val);
                Task::none()
            }
            Message::ImportSettings => Task::perform(
                tokio::task::spawn_blocking(|| {
                    let path = rfd::FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .pick_file();
                    path.map(|p| Preferences::from_path(&p).map_err(|e| e.to_string()))
                        .transpose()
                }),
                |result| match result {
                    Ok(inner) => Message::ImportResult(inner),
                    Err(err) => Message::ImportResult(Err(err.to_string())),
                },
            ),
            Message::ImportResult(result) => {
                match result {
                    Ok(Some(prefs)) => {
                        self.apply(prefs);
                        self.discard_edits();
                        self.io_error = None;
                        self.import_status = Some("Preferences importees avec succes".to_string());
                    }
                    Ok(None) => {}
                    Err(e) => {
                        self.import_status = None;
                        self.io_error = Some(format!("Erreur d'import : {e}"));
                    }
                }
                Task::none()
            }
            Message::ExportSettings => {
                let prefs = self.preferences.clone();
                Task::perform(
                    tokio::task::spawn_blocking(move || {
                        let path = rfd::FileDialog::new()
                            .add_filter("JSON", &["json"])
                            .set_file_name("gitspace-preferences.json")
                            .save_file();
                        path.map(|p| {
                            prefs
                                .save_to_path(&p)
                                .map(|_| p.display().to_string())
                                .map_err(|e| e.to_string())
                        })
                        .transpose()
                    }),
                    |result| match result {
                        Ok(inner) => Message::ExportResult(inner),
                        Err(err) => Message::ExportResult(Err(err.to_string())),
                    },
                )
            }
            Message::ExportResult(result) => {
                match result {
                    Ok(Some(path)) => {
                        self.io_error = None;
                        self.export_status = Some(format!("Preferences exportees dans {path}"));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        self.export_status = None;
                        self.io_error = Some(format!("Erreur d'export : {e}"));
                    }
                }
                Task::none()
            }

            // -- Actions globales --
            Message::SavePreferences => {
                self.apply(self.preferences.clone());
                self.applied_logging = self.logging;
                self.pending_logging = Some(self.logging);
                Task::none()
            }
            Message::ResetDefaults => {
                self.confirm_reset = true;
                Task::none()
            }
            Message::CancelReset => {
                self.confirm_reset = false;
                Task::none()
            }
            Message::ConfirmReset => {
                self.apply(Preferences::default());
                self.applied_logging = LoggingOptions::default();
                self.pending_logging = Some(self.applied_logging);
                self.discard_edits();
                Task::none()
            }
        }
    }

    // ─── View ─────────────────────────────────────────────────────────

    /// Construit la vue plein ecran du panneau de preferences.
    /// Retourne `None` si le panneau est ferme.
    pub fn view<'a>(&'a self, theme: &'a Theme) -> Option<Element<'a, Message>> {
        if !self.open {
            return None;
        }
        let palette = &theme.palette;

        // Sidebar gauche
        let sidebar = self.view_sidebar(theme);

        // Contenu a droite selon la categorie
        let content_area = match self.active_category {
            PreferencesCategory::General => self.view_general(theme),
            PreferencesCategory::Appearance => self.view_appearance(theme),
            PreferencesCategory::GitSpace => self.view_gitspace(theme),
        };

        // Boutons Sauvegarder / Reinitialiser en bas du contenu
        let actions = self.view_content_actions(theme);

        let right_panel = column![
            scrollable(
                container(content_area)
                    .width(Length::Fill)
                    .padding(theme.spacing.lg)
            )
            .width(Length::Fill)
            .height(Length::Fill),
            container(actions)
                .width(Length::Fill)
                .padding([theme.spacing.sm, theme.spacing.lg]),
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        // Separateur vertical simule par un container colore
        let separator =
            container(Space::new().width(1))
                .height(Length::Fill)
                .style(move |_: &iced::Theme| container::Style {
                    background: Some(palette.surface_highlight.into()),
                    ..Default::default()
                });

        let layout = row![sidebar, separator, right_panel]
            .width(Length::Fill)
            .height(Length::Fill);

        let panel = container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(palette.background.into()),
                ..Default::default()
            });

        Some(panel.into())
    }

    // ─── Sidebar ──────────────────────────────────────────────────────

    /// Construit la sidebar avec les categories et le bouton fermer.
    fn view_sidebar<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let mut sidebar_items = column![
            Space::new().height(theme.spacing.sm),
            text("Preferences")
                .size(typo.title)
                .color(palette.text_primary),
            Space::new().height(theme.spacing.md),
        ]
        .spacing(2);

        // Boutons de categories
        for cat in PreferencesCategory::ALL {
            let is_selected = self.active_category == cat;
            sidebar_items = sidebar_items.push(self.view_sidebar_button(theme, cat, is_selected));
        }

        // Espace flexible pour pousser le bouton Fermer en bas
        sidebar_items = sidebar_items.push(Space::new().height(Length::Fill));

        // Bouton Fermer (abandonne les modifications non sauvegardees)
        let close_btn = button(text("Fermer").size(typo.body).color(palette.text_primary))
            .on_press(Message::Close)
            .padding([6, 12])
            .width(Length::Fill)
            .style(rounded_style(
                with_alpha(palette.surface_highlight, 0.3),
                with_alpha(palette.accent, 0.2),
                with_alpha(palette.accent, 0.3),
                palette.text_primary,
            ));

        sidebar_items = sidebar_items.push(close_btn);
        sidebar_items = sidebar_items.push(Space::new().height(theme.spacing.sm));

        container(sidebar_items.padding(theme.spacing.sm))
            .width(200)
            .height(Length::Fill)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(palette.surface.into()),
                ..Default::default()
            })
            .into()
    }

    /// Construit un bouton de sidebar pour une categorie.
    fn view_sidebar_button<'a>(
        &'a self,
        theme: &'a Theme,
        category: PreferencesCategory,
        is_selected: bool,
    ) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let text_color = if is_selected {
            palette.accent
        } else {
            palette.text_primary
        };
        let accent = palette.accent;
        let surface_hl = palette.surface_highlight;

        let label_text = format!("{} {}", category.icon(), category.label());

        let content = row![
            // Indicateur accent a gauche si selectionne
            if is_selected {
                container(Space::new().width(3))
                    .height(24)
                    .style(move |_: &iced::Theme| container::Style {
                        background: Some(accent.into()),
                        border: iced::Border {
                            radius: 2.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
            } else {
                container(Space::new().width(3)).height(24)
            },
            Space::new().width(6),
            text(label_text).size(typo.body).color(text_color),
        ]
        .align_y(Alignment::Center);

        let selected_bg = if is_selected {
            with_alpha(surface_hl, 0.4)
        } else {
            Color::TRANSPARENT
        };

        let hovered = if is_selected {
            selected_bg
        } else {
            with_alpha(surface_hl, 0.25)
        };

        button(content)
            .on_press(Message::SelectCategory(category))
            .padding([6, 8])
            .width(Length::Fill)
            .style(rounded_style(
                selected_bg,
                hovered,
                with_alpha(surface_hl, 0.5),
                text_color,
            ))
            .into()
    }

    // ─── Boutons d'actions bas du contenu ──────────────────────────────

    /// Boutons Sauvegarder et Reinitialiser en bas du contenu. La reinitialisation demande
    /// une confirmation.
    fn view_content_actions<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;
        let accent = palette.accent;
        let surface_hl = palette.surface_highlight;

        let save_btn = button(
            text("Sauvegarder")
                .size(typo.body)
                .color(palette.background),
        )
        .on_press(Message::SavePreferences)
        .padding([8, 16])
        .style(rounded_style(
            accent,
            with_alpha(accent, 0.85),
            with_alpha(accent, 0.7),
            palette.background,
        ));

        let mut actions = row![save_btn]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        if self.confirm_reset {
            actions = actions
                .push(
                    text("Remettre toutes les preferences par defaut ?")
                        .size(typo.body)
                        .color(palette.text_primary),
                )
                .push(styled_secondary_button(
                    theme,
                    "Confirmer",
                    Message::ConfirmReset,
                ))
                .push(styled_secondary_button(
                    theme,
                    "Annuler",
                    Message::CancelReset,
                ));
        } else {
            actions = actions.push(
                button(
                    text("Reinitialiser")
                        .size(typo.body)
                        .color(palette.text_primary),
                )
                .on_press(Message::ResetDefaults)
                .padding([8, 16])
                .style(rounded_style(
                    with_alpha(surface_hl, 0.2),
                    with_alpha(surface_hl, 0.4),
                    with_alpha(surface_hl, 0.6),
                    palette.text_primary,
                )),
            );
        }

        actions.into()
    }

    // ─── Contenu : General ────────────────────────────────────────────

    /// Section General : chemin de clone, options reseau.
    fn view_general<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let heading = text("General")
            .size(typo.heading)
            .color(palette.text_primary);

        let subtitle = text("Chemin de clone par defaut et options reseau")
            .size(typo.body)
            .color(palette.text_secondary);

        // -- Chemin de clone --
        let clone_label = text("Destination par defaut")
            .size(typo.body)
            .color(palette.text_secondary);

        let clone_input = text_input("/home/me/code", self.preferences.default_clone_path())
            .on_input(Message::DefaultClonePathChanged)
            .width(Length::Fixed(300.0));

        let browse_btn = styled_secondary_button(theme, "Choisir", Message::BrowseClonePath);

        let clone_row = row![clone_label, clone_input, browse_btn]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        // -- Reseau --
        let network = self.preferences.network();

        let http_proxy_label = text("Proxy HTTP")
            .size(typo.body)
            .color(palette.text_secondary);
        let http_proxy_input = text_input("http://proxy:8080", &network.http_proxy)
            .on_input(Message::HttpProxyChanged)
            .width(Length::Fixed(260.0));
        let http_proxy_row = row![http_proxy_label, http_proxy_input]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        let https_proxy_label = text("Proxy HTTPS")
            .size(typo.body)
            .color(palette.text_secondary);
        let https_proxy_input = text_input("https://proxy:8443", &network.https_proxy)
            .on_input(Message::HttpsProxyChanged)
            .width(Length::Fixed(260.0));
        let https_proxy_row = row![https_proxy_label, https_proxy_input]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        let timeout_label = text("Timeout (sec)")
            .size(typo.body)
            .color(palette.text_secondary);
        let timeout_input = text_input("30", &self.timeout_buffer)
            .on_input(Message::NetworkTimeoutChanged)
            .width(Length::Fixed(80.0));
        let timeout_row = row![timeout_label, timeout_input]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        let use_https_cb = checkbox(network.use_https)
            .label("Preferer HTTPS")
            .on_toggle(Message::UseHttpsChanged);
        let allow_ssh_cb = checkbox(network.allow_ssh)
            .label("Autoriser SSH")
            .on_toggle(Message::AllowSshChanged);
        let proto_row = row![
            use_https_cb,
            Space::new().width(theme.spacing.md),
            allow_ssh_cb
        ]
        .align_y(Alignment::Center);

        let auto_fetch_cb = checkbox(self.preferences.auto_fetch_enabled())
            .label("Recuperation automatique des remotes")
            .on_toggle(Message::AutoFetchEnabledChanged);

        let mut network_col = column![
            section_header(theme, "Reseau", "Proxy, timeout et protocoles"),
            Space::new().height(theme.spacing.xs),
            http_proxy_row,
            https_proxy_row,
            timeout_row,
            proto_row,
            auto_fetch_cb,
        ]
        .spacing(theme.spacing.sm);

        // Intervalle de fetch si active
        if self.preferences.auto_fetch_enabled() {
            let interval_label = text("Intervalle (min)")
                .size(typo.body)
                .color(palette.text_secondary);
            let interval_input = text_input("5", &self.fetch_interval_buffer)
                .on_input(Message::AutoFetchIntervalChanged)
                .width(Length::Fixed(80.0));
            let interval_row = row![interval_label, interval_input]
                .spacing(theme.spacing.sm)
                .align_y(Alignment::Center);
            network_col = network_col.push(interval_row);
        }

        column![
            heading,
            subtitle,
            Space::new().height(theme.spacing.md),
            section_header(
                theme,
                "Depots",
                "Chemin de destination par defaut pour les clones"
            ),
            Space::new().height(theme.spacing.xs),
            clone_row,
            Space::new().height(theme.spacing.md),
            rule::horizontal(1),
            Space::new().height(theme.spacing.md),
            network_col,
        ]
        .spacing(theme.spacing.sm)
        .width(Length::Fill)
        .into()
    }

    // ─── Contenu : Apparences ─────────────────────────────────────────

    /// Section Apparences : selection du theme.
    fn view_appearance<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let heading = text("Apparences")
            .size(typo.heading)
            .color(palette.text_primary);

        let subtitle = text("Personnalisez l'apparence de GitSpace")
            .size(typo.body)
            .color(palette.text_secondary);

        // Theme
        let theme_label = text("Theme Catppuccin")
            .size(typo.body)
            .color(palette.text_secondary);

        let theme_pick = pick_list(
            THEME_MODES,
            Some(self.preferences.theme_mode()),
            Message::ThemeModeChanged,
        )
        .width(Length::Fixed(160.0));

        let theme_row = row![theme_label, theme_pick]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        column![
            heading,
            subtitle,
            Space::new().height(theme.spacing.md),
            section_header(theme, "Theme", "Choisissez un theme Catppuccin"),
            Space::new().height(theme.spacing.xs),
            theme_row,
        ]
        .spacing(theme.spacing.sm)
        .width(Length::Fill)
        .into()
    }

    // ─── Contenu : GitSpace ───────────────────────────────────────────

    /// Section GitSpace : journalisation, import/export, mises a jour, reinitialisation.
    fn view_gitspace<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let heading = text("GitSpace")
            .size(typo.heading)
            .color(palette.text_primary);

        let subtitle = text("Parametres specifiques a GitSpace")
            .size(typo.body)
            .color(palette.text_secondary);

        // -- Journalisation --
        let retention_label = text("Fichiers de log conserves")
            .size(typo.body)
            .color(palette.text_secondary);

        let retention_slider = slider(
            MIN_LOG_RETENTION_FILES as f32..=MAX_LOG_RETENTION_FILES as f32,
            self.logging.retention_files() as f32,
            Message::RetentionFilesChanged,
        )
        .width(Length::Fixed(200.0));

        let retention_value = text(format!("{}", self.logging.retention_files()))
            .size(typo.label)
            .color(palette.text_secondary);

        let retention_row = row![retention_label, retention_slider, retention_value]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        // -- Confidentialite --
        let encrypted_cb = checkbox(self.preferences.allow_encrypted_tokens())
            .label("Autoriser le stockage chiffre des tokens")
            .on_toggle(Message::AllowEncryptedTokensChanged);

        // -- Import / Export --
        let import_btn = styled_secondary_button(theme, "Importer", Message::ImportSettings);

        let export_btn = styled_secondary_button(theme, "Exporter", Message::ExportSettings);

        let mut ie_col =
            column![
                row![import_btn, Space::new().width(theme.spacing.sm), export_btn]
                    .align_y(Alignment::Center),
            ]
            .spacing(theme.spacing.xs);

        if let Some(status) = &self.import_status {
            ie_col = ie_col.push(
                text(status.as_str())
                    .size(typo.label)
                    .color(palette.text_secondary),
            );
        }
        if let Some(status) = &self.export_status {
            ie_col = ie_col.push(
                text(status.as_str())
                    .size(typo.label)
                    .color(palette.text_secondary),
            );
        }
        if let Some(error) = &self.io_error {
            ie_col = ie_col.push(text(error.as_str()).size(typo.body).style(text::danger));
        }

        column![
            heading,
            subtitle,
            Space::new().height(theme.spacing.md),
            section_header(
                theme,
                "Journalisation",
                "Nombre de fichiers de log conserves"
            ),
            Space::new().height(theme.spacing.xs),
            retention_row,
            Space::new().height(theme.spacing.md),
            rule::horizontal(1),
            Space::new().height(theme.spacing.md),
            section_header(theme, "Confidentialite", "Stockage securise des tokens"),
            Space::new().height(theme.spacing.xs),
            encrypted_cb,
            Space::new().height(theme.spacing.md),
            rule::horizontal(1),
            Space::new().height(theme.spacing.md),
            section_header(
                theme,
                "Import / Export",
                "Transferez vos preferences au format JSON"
            ),
            Space::new().height(theme.spacing.xs),
            ie_col,
        ]
        .spacing(theme.spacing.sm)
        .width(Length::Fill)
        .into()
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────

/// En-tete de sous-section avec barre accent a gauche.
fn section_header<'a, M: 'a>(
    theme: &'a Theme,
    title: &'a str,
    subtitle: &'a str,
) -> Element<'a, M> {
    let palette = &theme.palette;
    let typo = &theme.typography;
    let accent = palette.accent;

    let indicator = container(Space::new().width(3))
        .height(32)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(accent.into()),
            border: iced::Border {
                radius: 2.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    let labels = column![
        text(title).size(typo.body).color(palette.text_primary),
        text(subtitle)
            .size(typo.label)
            .color(palette.text_secondary),
    ]
    .spacing(2);

    row![indicator, Space::new().width(8), labels]
        .align_y(Alignment::Center)
        .into()
}

/// Cree un bouton secondaire style Colony (fond surface_highlight, coins arrondis).
fn styled_secondary_button<'a>(
    theme: &'a Theme,
    label: &'a str,
    on_press: Message,
) -> Element<'a, Message> {
    let palette = &theme.palette;
    let surface_hl = palette.surface_highlight;

    button(
        text(label)
            .size(theme.typography.body)
            .color(palette.text_primary),
    )
    .on_press(on_press)
    .padding([6, 12])
    .style(rounded_style(
        with_alpha(surface_hl, 0.3),
        with_alpha(surface_hl, 0.5),
        with_alpha(surface_hl, 0.7),
        palette.text_primary,
    ))
    .into()
}

/// Style de bouton a coins arrondis, avec un fond par etat (repos, survol, appui).
fn rounded_style(
    idle: Color,
    hovered: Color,
    pressed: Color,
    text_color: Color,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: Some(
            match status {
                button::Status::Hovered => hovered,
                button::Status::Pressed => pressed,
                _ => idle,
            }
            .into(),
        ),
        border: iced::Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        text_color,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_panel() -> PreferencesPanel {
        let mut panel = PreferencesPanel::new(Preferences::default(), LoggingOptions::default());
        panel.toggle();
        panel
    }

    #[test]
    fn theme_change_applies_only_the_theme() {
        let mut panel = open_panel();
        let _ = panel.update(Message::AutoFetchEnabledChanged(true));
        let _ = panel.update(Message::ThemeModeChanged(ThemeMode::Latte));

        let applied = panel.take_changes().expect("theme applied");
        assert_eq!(applied.theme_mode(), ThemeMode::Latte);
        assert!(!applied.auto_fetch_enabled());
        // The app hands the applied value back; the unsaved edit stays on screen.
        panel.set_preferences(applied);
        assert!(panel.preferences.auto_fetch_enabled());
    }

    #[test]
    fn close_discards_unsaved_edits_and_reset_waits_for_confirmation() {
        let mut panel = open_panel();
        let _ = panel.update(Message::AutoFetchEnabledChanged(true));
        let _ = panel.update(Message::Close);
        assert!(!panel.preferences.auto_fetch_enabled());
        assert!(panel.take_changes().is_none());

        panel.toggle();
        let _ = panel.update(Message::ResetDefaults);
        assert!(panel.take_changes().is_none());
        let _ = panel.update(Message::ConfirmReset);
        assert_eq!(panel.take_changes(), Some(Preferences::default()));
    }
}
