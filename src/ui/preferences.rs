//! Panneau de preferences plein ecran avec sidebar et categories.
//!
//! Remplace le contenu principal quand ouvert. Utilise une sidebar
//! gauche avec 4 categories et un contenu scrollable a droite.

use iced::widget::{
    button, checkbox, column, container, horizontal_rule, pick_list, row, scrollable, slider, text,
    text_input, Space,
};
use iced::{Alignment, Color, Element, Length, Task};

use crate::config::{
    LoggingOptions, MotionIntensity, Preferences, ReleaseChannel, ThemeMode,
    MAX_LOG_RETENTION_FILES, MIN_LOG_RETENTION_FILES,
};
use crate::ui::theme::{with_alpha, Theme};

/// Modes de theme disponibles pour le pick_list.
const THEME_MODES: &[ThemeMode] = &[
    ThemeMode::Latte,
    ThemeMode::Frappe,
    ThemeMode::Macchiato,
    ThemeMode::Mocha,
];

/// Canaux de release disponibles.
const RELEASE_CHANNELS: &[ReleaseChannel] = &[ReleaseChannel::Stable, ReleaseChannel::Preview];

/// Intensites de mouvement disponibles.
const MOTION_INTENSITIES: &[MotionIntensity] = &[
    MotionIntensity::Low,
    MotionIntensity::Medium,
    MotionIntensity::High,
];

// ─── Categories ───────────────────────────────────────────────────────────

/// Categorie de la sidebar preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferencesCategory {
    General,
    Appearance,
    Accessibility,
    GitSpace,
}

impl PreferencesCategory {
    pub const ALL: [Self; 4] = [
        Self::General,
        Self::Appearance,
        Self::Accessibility,
        Self::GitSpace,
    ];

    /// Libelle affiche dans la sidebar.
    pub fn label(&self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Apparences",
            Self::Accessibility => "Accessibilite",
            Self::GitSpace => "GitSpace",
        }
    }

    /// Icone affichee devant le libelle.
    pub fn icon(&self) -> &'static str {
        match self {
            Self::General => "\u{2699}",       // ⚙
            Self::Appearance => "\u{1f3a8}",   // 🎨
            Self::Accessibility => "\u{267f}", // ♿
            Self::GitSpace => "\u{1f680}",     // 🚀
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
    /// Hauteur des controles modifiee.
    ControlHeightChanged(f32),

    // -- Accessibilite --
    /// Intensite de mouvement modifiee.
    MotionIntensityChanged(MotionIntensity),
    /// Mouvement reduit active/desactive.
    ReducedMotionChanged(bool),
    /// Mode performance active/desactive.
    PerformanceModeChanged(bool),

    // -- GitSpace --
    /// Nombre de fichiers de log conserves modifie.
    RetentionFilesChanged(f32),
    /// Stockage chiffre autorise/desautorise.
    AllowEncryptedTokensChanged(bool),
    /// Verification automatique des mises a jour modifiee.
    AutoCheckUpdatesChanged(bool),
    /// Canal de release modifie.
    ReleaseChannelChanged(ReleaseChannel),
    /// Feed de mise a jour personnalise modifie.
    UpdateFeedOverrideChanged(String),
    /// Verifier les mises a jour maintenant.
    CheckForUpdates,
    /// Importer les parametres.
    ImportSettings,
    /// Resultat de l'import.
    ImportResult(Result<Preferences, String>),
    /// Exporter les parametres.
    ExportSettings,
    /// Resultat de l'export.
    ExportResult(Result<String, String>),

    // -- Actions globales --
    /// Sauvegarder les preferences.
    SavePreferences,
    /// Reinitialiser les preferences par defaut.
    ResetDefaults,
}

// ─── Etat ─────────────────────────────────────────────────────────────────

/// Panneau de preferences plein ecran.
pub struct PreferencesPanel {
    /// Preferences courantes en cours d'edition.
    preferences: Preferences,
    /// Options de journalisation courantes.
    logging: LoggingOptions,
    /// Preferences en attente d'application par l'app.
    pending_preferences: Option<Preferences>,
    /// Journalisation en attente d'application.
    pending_logging: Option<LoggingOptions>,
    /// Demande de verification de mise a jour.
    update_request: bool,
    /// Statut de mise a jour affiche.
    update_status: Option<String>,
    /// Statut import/export.
    import_status: Option<String>,
    export_status: Option<String>,
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
            preferences,
            logging,
            pending_preferences: None,
            pending_logging: None,
            update_request: false,
            update_status: None,
            import_status: None,
            export_status: None,
            open: false,
            active_category: PreferencesCategory::General,
            timeout_buffer,
            fetch_interval_buffer,
        }
    }

    /// Met a jour les preferences depuis l'exterieur.
    pub fn set_preferences(&mut self, preferences: Preferences) {
        self.timeout_buffer = preferences.network().network_timeout_secs.to_string();
        self.fetch_interval_buffer = preferences.auto_fetch_interval_minutes().to_string();
        self.preferences = preferences;
    }

    /// Le panneau est-il ouvert ?
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Bascule l'ouverture du panneau.
    pub fn toggle(&mut self) {
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

    /// Indique si une verification de mise a jour a ete demandee.
    pub fn take_update_request(&mut self) -> bool {
        if self.update_request {
            self.update_request = false;
            return true;
        }
        false
    }

    /// Definit le statut de mise a jour affiche.
    pub fn set_update_status<S: Into<String>>(&mut self, status: S) {
        self.update_status = Some(status.into());
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
                self.preferences.set_theme_mode(mode);
                // Applique immediatement le changement de theme
                self.pending_preferences = Some(self.preferences.clone());
                Task::none()
            }
            Message::ControlHeightChanged(height) => {
                self.preferences.set_control_height(height);
                Task::none()
            }

            // -- Accessibilite --
            Message::MotionIntensityChanged(intensity) => {
                self.preferences.set_motion_intensity(intensity);
                Task::none()
            }
            Message::ReducedMotionChanged(val) => {
                self.preferences.set_reduced_motion(val);
                Task::none()
            }
            Message::PerformanceModeChanged(val) => {
                self.preferences.set_performance_mode(val);
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
            Message::AutoCheckUpdatesChanged(val) => {
                self.preferences.set_auto_check_updates(val);
                Task::none()
            }
            Message::ReleaseChannelChanged(channel) => {
                self.preferences.set_release_channel(channel);
                Task::none()
            }
            Message::UpdateFeedOverrideChanged(val) => {
                self.preferences.set_update_feed_override(Some(val));
                Task::none()
            }
            Message::CheckForUpdates => {
                self.update_request = true;
                self.update_status = Some("Verification des mises a jour...".to_string());
                Task::none()
            }
            Message::ImportSettings => Task::perform(
                tokio::task::spawn_blocking(|| {
                    let path = rfd::FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .pick_file();
                    match path {
                        Some(p) => {
                            Preferences::from_path(&p).map_err(|e| e.to_string())
                        }
                        None => Err("Import annule".to_string()),
                    }
                }),
                |result| match result {
                    Ok(inner) => Message::ImportResult(inner),
                    Err(err) => Message::ImportResult(Err(err.to_string())),
                },
            ),
            Message::ImportResult(result) => {
                match result {
                    Ok(prefs) => {
                        self.preferences = prefs.clone();
                        self.pending_preferences = Some(prefs);
                        self.import_status =
                            Some("Preferences importees avec succes".to_string());
                    }
                    Err(e) => {
                        self.import_status = Some(format!("Erreur d'import : {e}"));
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
                        match path {
                            Some(p) => prefs
                                .save_to_path(&p)
                                .map(|_| p.display().to_string())
                                .map_err(|e| e.to_string()),
                            None => Err("Export annule".to_string()),
                        }
                    }),
                    |result| match result {
                        Ok(inner) => Message::ExportResult(inner),
                        Err(err) => Message::ExportResult(Err(err.to_string())),
                    },
                )
            }
            Message::ExportResult(result) => {
                match result {
                    Ok(path) => {
                        self.export_status =
                            Some(format!("Preferences exportees dans {path}"));
                    }
                    Err(e) => {
                        self.export_status = Some(format!("Erreur d'export : {e}"));
                    }
                }
                Task::none()
            }

            // -- Actions globales --
            Message::SavePreferences => {
                self.pending_preferences = Some(self.preferences.clone());
                self.pending_logging = Some(self.logging);
                Task::none()
            }
            Message::ResetDefaults => {
                self.preferences = Preferences::default();
                self.logging = LoggingOptions::default();
                self.timeout_buffer =
                    self.preferences.network().network_timeout_secs.to_string();
                self.fetch_interval_buffer =
                    self.preferences.auto_fetch_interval_minutes().to_string();
                self.pending_preferences = Some(self.preferences.clone());
                self.pending_logging = Some(self.logging);
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
            PreferencesCategory::Accessibility => self.view_accessibility(theme),
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
        let separator = container(Space::with_width(1))
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
            Space::with_height(theme.spacing.sm),
            text("Preferences")
                .size(typo.title)
                .color(palette.text_primary),
            Space::with_height(theme.spacing.md),
        ]
        .spacing(2);

        // Boutons de categories
        for cat in PreferencesCategory::ALL {
            let is_selected = self.active_category == cat;
            sidebar_items = sidebar_items.push(
                self.view_sidebar_button(theme, cat, is_selected),
            );
        }

        // Espace flexible pour pousser le bouton Fermer en bas
        sidebar_items = sidebar_items.push(Space::with_height(Length::Fill));

        // Bouton Fermer
        let close_accent = palette.accent;
        let close_text_color = palette.text_primary;
        let close_surface = palette.surface_highlight;
        let close_btn = button(
            text("Fermer")
                .size(typo.body)
                .color(close_text_color),
        )
        .on_press(Message::Close)
        .padding([6, 12])
        .width(Length::Fill)
        .style(move |_: &iced::Theme, status| {
            let bg = match status {
                button::Status::Hovered => with_alpha(close_accent, 0.2),
                button::Status::Pressed => with_alpha(close_accent, 0.3),
                _ => with_alpha(close_surface, 0.3),
            };
            button::Style {
                background: Some(bg.into()),
                border: iced::Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                text_color: close_text_color,
                ..Default::default()
            }
        });

        sidebar_items = sidebar_items.push(close_btn);
        sidebar_items = sidebar_items.push(Space::with_height(theme.spacing.sm));

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
                container(Space::with_width(3))
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
                container(Space::with_width(3)).height(24)
            },
            Space::with_width(6),
            text(label_text)
                .size(typo.body)
                .color(text_color),
        ]
        .align_y(Alignment::Center);

        let selected_bg = if is_selected {
            with_alpha(surface_hl, 0.4)
        } else {
            Color::TRANSPARENT
        };

        button(content)
            .on_press(Message::SelectCategory(category))
            .padding([6, 8])
            .width(Length::Fill)
            .style(move |_: &iced::Theme, status| {
                let bg = match status {
                    button::Status::Hovered if !is_selected => {
                        with_alpha(surface_hl, 0.25)
                    }
                    button::Status::Pressed => with_alpha(surface_hl, 0.5),
                    _ => selected_bg,
                };
                button::Style {
                    background: Some(bg.into()),
                    border: iced::Border {
                        radius: 6.0.into(),
                        ..Default::default()
                    },
                    text_color,
                    ..Default::default()
                }
            })
            .into()
    }

    // ─── Boutons d'actions bas du contenu ──────────────────────────────

    /// Boutons Sauvegarder et Reinitialiser en bas du contenu.
    fn view_content_actions<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let accent = palette.accent;
        let surface_hl = palette.surface_highlight;
        let text_primary = palette.text_primary;
        let bg_color = palette.background;

        let save_btn = button(
            text("Sauvegarder")
                .size(typo.body)
                .color(bg_color),
        )
        .on_press(Message::SavePreferences)
        .padding([8, 16])
        .style(move |_: &iced::Theme, status| {
            let bg = match status {
                button::Status::Hovered => with_alpha(accent, 0.85),
                button::Status::Pressed => with_alpha(accent, 0.7),
                _ => accent,
            };
            button::Style {
                background: Some(bg.into()),
                border: iced::Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                text_color: bg_color,
                ..Default::default()
            }
        });

        let reset_btn = button(
            text("Reinitialiser")
                .size(typo.body)
                .color(text_primary),
        )
        .on_press(Message::ResetDefaults)
        .padding([8, 16])
        .style(move |_: &iced::Theme, status| {
            let bg = match status {
                button::Status::Hovered => with_alpha(surface_hl, 0.4),
                button::Status::Pressed => with_alpha(surface_hl, 0.6),
                _ => with_alpha(surface_hl, 0.2),
            };
            button::Style {
                background: Some(bg.into()),
                border: iced::Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                text_color: text_primary,
                ..Default::default()
            }
        });

        row![save_btn, Space::with_width(theme.spacing.sm), reset_btn]
            .align_y(Alignment::Center)
            .into()
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

        let browse_btn = styled_secondary_button(
            theme,
            "Choisir",
            Message::BrowseClonePath,
        );

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

        let use_https_cb = checkbox("Preferer HTTPS", network.use_https)
            .on_toggle(Message::UseHttpsChanged);
        let allow_ssh_cb = checkbox("Autoriser SSH", network.allow_ssh)
            .on_toggle(Message::AllowSshChanged);
        let proto_row = row![use_https_cb, Space::with_width(theme.spacing.md), allow_ssh_cb]
            .align_y(Alignment::Center);

        let auto_fetch_cb =
            checkbox("Recuperation automatique des remotes", self.preferences.auto_fetch_enabled())
                .on_toggle(Message::AutoFetchEnabledChanged);

        let mut network_col = column![
            section_header(theme, "Reseau", "Proxy, timeout et protocoles"),
            Space::with_height(theme.spacing.xs),
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
            Space::with_height(theme.spacing.md),
            section_header(theme, "Depots", "Chemin de destination par defaut pour les clones"),
            Space::with_height(theme.spacing.xs),
            clone_row,
            Space::with_height(theme.spacing.md),
            horizontal_rule(1),
            Space::with_height(theme.spacing.md),
            network_col,
        ]
        .spacing(theme.spacing.sm)
        .width(Length::Fill)
        .into()
    }

    // ─── Contenu : Apparences ─────────────────────────────────────────

    /// Section Apparences : selection du theme, hauteur des controles.
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

        // Hauteur des controles
        let height_label = text("Hauteur des controles")
            .size(typo.body)
            .color(palette.text_secondary);

        let height_slider = slider(
            20.0..=48.0,
            self.preferences.control_height(),
            Message::ControlHeightChanged,
        )
        .width(Length::Fixed(200.0));

        let height_value = text(format!("{:.0}px", self.preferences.control_height()))
            .size(typo.label)
            .color(palette.text_secondary);

        let height_row = row![height_label, height_slider, height_value]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        column![
            heading,
            subtitle,
            Space::with_height(theme.spacing.md),
            section_header(theme, "Theme", "Choisissez un theme Catppuccin"),
            Space::with_height(theme.spacing.xs),
            theme_row,
            Space::with_height(theme.spacing.md),
            horizontal_rule(1),
            Space::with_height(theme.spacing.md),
            section_header(theme, "Taille des controles", "Ajustez la hauteur des elements"),
            Space::with_height(theme.spacing.xs),
            height_row,
        ]
        .spacing(theme.spacing.sm)
        .width(Length::Fill)
        .into()
    }

    // ─── Contenu : Accessibilite ──────────────────────────────────────

    /// Section Accessibilite : mouvement reduit, intensite, mode performance.
    fn view_accessibility<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let heading = text("Accessibilite")
            .size(typo.heading)
            .color(palette.text_primary);

        let subtitle = text("Options d'accessibilite et de mouvement")
            .size(typo.body)
            .color(palette.text_secondary);

        // Intensite de mouvement
        let motion_label = text("Intensite des animations")
            .size(typo.body)
            .color(palette.text_secondary);

        let motion_pick = pick_list(
            MOTION_INTENSITIES,
            Some(self.preferences.motion_intensity()),
            Message::MotionIntensityChanged,
        )
        .width(Length::Fixed(160.0));

        let motion_row = row![motion_label, motion_pick]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        // Checkboxes
        let reduced_cb = checkbox("Reduire les animations", self.preferences.reduced_motion())
            .on_toggle(Message::ReducedMotionChanged);

        let perf_cb = checkbox("Mode performance", self.preferences.performance_mode())
            .on_toggle(Message::PerformanceModeChanged);

        column![
            heading,
            subtitle,
            Space::with_height(theme.spacing.md),
            section_header(theme, "Animations", "Controlez l'intensite et le comportement des animations"),
            Space::with_height(theme.spacing.xs),
            motion_row,
            reduced_cb,
            perf_cb,
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
        let encrypted_cb = checkbox(
            "Autoriser le stockage chiffre des tokens",
            self.preferences.allow_encrypted_tokens(),
        )
        .on_toggle(Message::AllowEncryptedTokensChanged);

        // -- Mises a jour --
        let auto_update_cb = checkbox(
            "Verifier automatiquement les mises a jour",
            self.preferences.auto_check_updates(),
        )
        .on_toggle(Message::AutoCheckUpdatesChanged);

        let channel_label = text("Canal de release")
            .size(typo.body)
            .color(palette.text_secondary);

        let channel_pick = pick_list(
            RELEASE_CHANNELS,
            Some(self.preferences.release_channel()),
            Message::ReleaseChannelChanged,
        )
        .width(Length::Fixed(140.0));

        let channel_row = row![channel_label, channel_pick]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        let feed_label = text("Feed URL personnalise")
            .size(typo.body)
            .color(palette.text_secondary);
        let feed_value = self
            .preferences
            .update_feed_override()
            .unwrap_or_default()
            .to_string();
        let feed_input = text_input("https://example.com/feed.json", &feed_value)
            .on_input(Message::UpdateFeedOverrideChanged)
            .width(Length::Fixed(300.0));
        let feed_row = row![feed_label, feed_input]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        let check_btn = styled_secondary_button(
            theme,
            "Verifier maintenant",
            Message::CheckForUpdates,
        );

        let mut update_row = row![check_btn].spacing(theme.spacing.sm).align_y(Alignment::Center);
        if let Some(status) = &self.update_status {
            update_row = update_row.push(
                text(status.as_str())
                    .size(typo.label)
                    .color(palette.text_secondary),
            );
        }

        // -- Import / Export --
        let import_btn = styled_secondary_button(
            theme,
            "Importer",
            Message::ImportSettings,
        );

        let export_btn = styled_secondary_button(
            theme,
            "Exporter",
            Message::ExportSettings,
        );

        let mut ie_col = column![
            row![import_btn, Space::with_width(theme.spacing.sm), export_btn]
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

        column![
            heading,
            subtitle,
            Space::with_height(theme.spacing.md),
            section_header(theme, "Journalisation", "Nombre de fichiers de log conserves"),
            Space::with_height(theme.spacing.xs),
            retention_row,
            Space::with_height(theme.spacing.md),
            horizontal_rule(1),
            Space::with_height(theme.spacing.md),
            section_header(theme, "Confidentialite", "Stockage securise des tokens"),
            Space::with_height(theme.spacing.xs),
            encrypted_cb,
            Space::with_height(theme.spacing.md),
            horizontal_rule(1),
            Space::with_height(theme.spacing.md),
            section_header(theme, "Mises a jour", "Verification et canal de release"),
            Space::with_height(theme.spacing.xs),
            auto_update_cb,
            channel_row,
            feed_row,
            update_row,
            Space::with_height(theme.spacing.md),
            horizontal_rule(1),
            Space::with_height(theme.spacing.md),
            section_header(theme, "Import / Export", "Transferez vos preferences au format JSON"),
            Space::with_height(theme.spacing.xs),
            ie_col,
        ]
        .spacing(theme.spacing.sm)
        .width(Length::Fill)
        .into()
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────

/// En-tete de sous-section avec barre accent a gauche.
fn section_header<'a, M: 'a>(theme: &'a Theme, title: &'a str, subtitle: &'a str) -> Element<'a, M> {
    let palette = &theme.palette;
    let typo = &theme.typography;
    let accent = palette.accent;

    let indicator = container(Space::with_width(3))
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
        text(title)
            .size(typo.body)
            .color(palette.text_primary),
        text(subtitle)
            .size(typo.label)
            .color(palette.text_secondary),
    ]
    .spacing(2);

    row![indicator, Space::with_width(8), labels]
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
    let typo = &theme.typography;
    let text_color = palette.text_primary;
    let surface_hl = palette.surface_highlight;

    button(text(label).size(typo.body).color(text_color))
        .on_press(on_press)
        .padding([6, 12])
        .style(move |_: &iced::Theme, status| {
            let bg = match status {
                button::Status::Hovered => with_alpha(surface_hl, 0.5),
                button::Status::Pressed => with_alpha(surface_hl, 0.7),
                _ => with_alpha(surface_hl, 0.3),
            };
            button::Style {
                background: Some(bg.into()),
                border: iced::Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                text_color,
                ..Default::default()
            }
        })
        .into()
}
