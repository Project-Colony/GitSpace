use std::sync::Arc;

use eframe::egui::{
    self, Align, ComboBox, Layout, Rect, RichText, Rounding, Sense, Slider, TextEdit, Ui, Vec2,
};
use rfd::FileDialog;

use crate::config::{
    Keybinding, LoggingOptions, MotionIntensity, Preferences, ThemeMode, MAX_LOG_RETENTION_FILES,
    MIN_LOG_RETENTION_FILES,
};
use crate::dotnet::{DialogOpenRequest, DialogOptions, DotnetClient};
use crate::ui::menu;
use crate::ui::notifications::{Notification, NotificationCenter};
use crate::ui::theme::SharedTheme;

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

    pub fn label(&self) -> &'static str {
        match self {
            Self::General => "Général",
            Self::Appearance => "Apparences",
            Self::Accessibility => "Accessibilité",
            Self::GitSpace => "GitSpace",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::General => "⚙",
            Self::Appearance => "🎨",
            Self::Accessibility => "♿",
            Self::GitSpace => "📦",
        }
    }
}

pub struct PreferencesPanel {
    theme: SharedTheme,
    preferences: Preferences,
    logging: LoggingOptions,
    pending_preferences: Option<Preferences>,
    pending_logging: Option<LoggingOptions>,
    pending_control_height: Option<f32>,
    import_status: Option<String>,
    export_status: Option<String>,
    native_dialog_status: Option<String>,
    open: bool,
    active_category: PreferencesCategory,
}

impl PreferencesPanel {
    pub fn new(theme: SharedTheme, preferences: Preferences, logging: LoggingOptions) -> Self {
        Self {
            theme,
            preferences,
            logging,
            pending_preferences: None,
            pending_logging: None,
            pending_control_height: None,
            import_status: None,
            export_status: None,
            native_dialog_status: None,
            open: false,
            active_category: PreferencesCategory::General,
        }
    }

    pub fn set_theme(&mut self, theme: SharedTheme) {
        self.theme = theme;
    }

    pub fn set_preferences(&mut self, preferences: Preferences) {
        self.preferences = preferences;
    }

    #[allow(dead_code)] // the egui shell drives `open` through `toggle` only
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    #[allow(dead_code)]
    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn take_changes(&mut self) -> Option<Preferences> {
        self.pending_preferences.take()
    }

    pub fn take_logging_changes(&mut self) -> Option<LoggingOptions> {
        self.pending_logging.take()
    }

    pub fn take_control_height_change(&mut self) -> Option<f32> {
        self.pending_control_height.take()
    }

    /// Mark preferences as changed (triggers auto-save)
    fn mark_changed(&mut self) {
        self.pending_preferences = Some(self.preferences.clone());
        self.pending_logging = Some(self.logging);
    }

    /// Show the preferences as a fullscreen panel (replaces all other content)
    /// Returns true if the panel is open (caller should skip rendering other panels)
    pub fn show(&mut self, ctx: &egui::Context, notifications: &mut NotificationCenter) -> bool {
        if !self.open {
            return false;
        }

        // Fullscreen panel that covers everything below the header
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(self.theme.palette.background))
            .show(ctx, |ui| {
                self.ui_content(ui, notifications);
            });

        // Close on escape key
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.open = false;
        }

        true
    }

    fn ui_content(&mut self, ui: &mut Ui, notifications: &mut NotificationCenter) {
        let available_size = ui.available_size();
        let sidebar_width = 180.0;

        ui.horizontal(|ui| {
            // Sidebar
            ui.vertical(|ui| {
                ui.set_width(sidebar_width);
                ui.set_min_height(available_size.y - 60.0);

                ui.add_space(8.0);

                for category in PreferencesCategory::ALL {
                    let is_selected = self.active_category == category;
                    self.sidebar_item(ui, category, is_selected);
                }

                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Fermer").clicked() {
                            self.open = false;
                        }
                    });
                });
            });

            // Separator
            ui.separator();

            // Content area
            ui.vertical(|ui| {
                ui.set_min_width(available_size.x - sidebar_width - 20.0);

                egui::ScrollArea::vertical()
                    .id_source("preferences_content")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(8.0);

                        match self.active_category {
                            PreferencesCategory::General => self.general_content(ui, notifications),
                            PreferencesCategory::Appearance => self.appearance_content(ui),
                            PreferencesCategory::Accessibility => self.accessibility_content(ui),
                            PreferencesCategory::GitSpace => {
                                self.gitspace_content(ui, notifications)
                            }
                        }

                        ui.add_space(16.0);
                    });
            });
        });
    }

    fn sidebar_item(&mut self, ui: &mut Ui, category: PreferencesCategory, is_selected: bool) {
        let item_height = 36.0;
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width() - 8.0, item_height),
            Sense::click(),
        );

        let bg_color = if is_selected {
            self.theme.palette.surface_highlight
        } else if response.hovered() {
            self.theme.palette.surface.linear_multiply(1.1)
        } else {
            egui::Color32::TRANSPARENT
        };

        ui.painter()
            .rect_filled(rect, Rounding::same(6.0), bg_color);

        let text_color = if is_selected {
            self.theme.palette.accent
        } else {
            self.theme.palette.text_primary
        };

        let icon_rect = Rect::from_min_size(
            rect.min + Vec2::new(12.0, (item_height - 16.0) / 2.0),
            Vec2::splat(16.0),
        );
        ui.painter().text(
            icon_rect.center(),
            egui::Align2::CENTER_CENTER,
            category.icon(),
            egui::FontId::proportional(14.0),
            text_color,
        );

        let text_pos = rect.min + Vec2::new(36.0, (item_height - 14.0) / 2.0);
        ui.painter().text(
            text_pos,
            egui::Align2::LEFT_TOP,
            category.label(),
            egui::FontId::proportional(14.0),
            text_color,
        );

        if is_selected {
            let indicator_rect = Rect::from_min_size(rect.min, Vec2::new(3.0, item_height));
            ui.painter()
                .rect_filled(indicator_rect, Rounding::ZERO, self.theme.palette.accent);
        }

        if response.clicked() {
            self.active_category = category;
        }

        ui.add_space(2.0);
    }

    // ========================
    // Général (General) section
    // ========================
    fn general_content(&mut self, ui: &mut Ui, _notifications: &mut NotificationCenter) {
        ui.heading(RichText::new("Général").color(self.theme.palette.text_primary));
        ui.label(
            RichText::new("Paramètres généraux de l'application")
                .color(self.theme.palette.text_secondary),
        );
        ui.add_space(16.0);

        // Privacy subsection
        self.section_header(
            ui,
            "Confidentialité",
            "Contrôlez le stockage des tokens et la sécurité",
        );
        ui.add_space(8.0);

        let mut encrypted_tokens = self.preferences.allow_encrypted_tokens();
        let response = ui.checkbox(
            &mut encrypted_tokens,
            "Autoriser le stockage chiffré si le trousseau natif n'est pas disponible",
        )
        .on_hover_text(
            "GitSpace utilise le trousseau du système par défaut. Activez cette option pour utiliser un fichier chiffré local si l'accès au trousseau échoue.",
        );
        if response.changed() {
            self.preferences
                .set_allow_encrypted_tokens(encrypted_tokens);
            self.mark_changed();
        }

        ui.add_space(20.0);

        // Logging subsection
        self.section_header(
            ui,
            "Journalisation",
            "Configurez le nombre de fichiers de log conservés",
        );
        ui.add_space(8.0);

        let mut retention_files = self.logging.retention_files() as u32;
        let response = ui.add(
            Slider::new(
                &mut retention_files,
                MIN_LOG_RETENTION_FILES as u32..=MAX_LOG_RETENTION_FILES as u32,
            )
            .text("Fichiers de log conservés"),
        );
        if response.changed() {
            self.logging.set_retention_files(retention_files as usize);
            self.mark_changed();
        }

        ui.add_space(20.0);

        // Import/Export subsection
        self.section_header(
            ui,
            "Import / Export",
            "Transférez vos préférences entre machines au format JSON",
        );
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if ui.button("Importer les paramètres").clicked() {
                if let Some(path) = FileDialog::new().add_filter("JSON", &["json"]).pick_file() {
                    match Preferences::from_path(&path) {
                        Ok(prefs) => {
                            self.preferences = prefs.clone();
                            self.pending_preferences = Some(prefs);
                            self.import_status =
                                Some(format!("Préférences importées depuis {}", path.display()));
                        }
                        Err(err) => {
                            self.import_status = Some(err.to_string());
                        }
                    }
                }
            }

            if ui.button("Exporter les paramètres").clicked() {
                if let Some(path) = FileDialog::new()
                    .add_filter("JSON", &["json"])
                    .set_file_name("gitspace-preferences.json")
                    .save_file()
                {
                    match self.preferences.save_to_path(&path) {
                        Ok(_) => {
                            self.export_status =
                                Some(format!("Préférences sauvegardées dans {}", path.display()));
                        }
                        Err(err) => self.export_status = Some(err.to_string()),
                    }
                }
            }
        });

        if let Some(status) = &self.import_status {
            ui.add_space(4.0);
            ui.label(RichText::new(status).color(self.theme.palette.text_secondary));
        }
        if let Some(status) = &self.export_status {
            ui.add_space(4.0);
            ui.label(RichText::new(status).color(self.theme.palette.text_secondary));
        }

        ui.add_space(20.0);

        // Reset button only (changes are auto-saved)
        if ui.button("Réinitialiser par défaut").clicked() {
            self.preferences = Preferences::default();
            self.logging = LoggingOptions::default();
            self.mark_changed();
        }
    }

    // ========================
    // Apparences (Appearance) section
    // ========================
    fn appearance_content(&mut self, ui: &mut Ui) {
        ui.heading(RichText::new("Apparences").color(self.theme.palette.text_primary));
        ui.label(
            RichText::new("Personnalisez l'apparence de GitSpace")
                .color(self.theme.palette.text_secondary),
        );
        ui.add_space(16.0);

        // Theme subsection
        self.section_header(
            ui,
            "Thème",
            "Choisissez un thème Catppuccin pour l'interface",
        );
        ui.add_space(8.0);

        let icon_id = ui.make_persistent_id("prefs-theme-icon");
        ComboBox::from_label(RichText::new("Thème").color(self.theme.palette.text_secondary))
            .selected_text(mode_label(self.preferences.theme_mode()))
            .icon(menu::combo_icon(Arc::clone(&self.theme), icon_id))
            .show_ui(ui, |ui| {
                menu::with_menu_popup_motion(ui, "prefs-theme-menu", |ui| {
                    let current_mode = self.preferences.theme_mode();
                    let mut selected_mode = current_mode;
                    for mode in [
                        ThemeMode::Latte,
                        ThemeMode::Frappe,
                        ThemeMode::Macchiato,
                        ThemeMode::Mocha,
                    ] {
                        if menu::menu_item(
                            ui,
                            &self.theme,
                            ("prefs-theme-item", mode_label(mode)),
                            mode_label(mode),
                            selected_mode == mode,
                        )
                        .clicked()
                        {
                            selected_mode = mode;
                        }
                    }
                    if selected_mode != current_mode {
                        self.preferences.set_theme_mode(selected_mode);
                        self.pending_preferences = Some(self.preferences.clone());
                    }
                });
            });

        ui.add_space(16.0);

        // Control height subsection
        self.section_header(
            ui,
            "Taille des contrôles",
            "Ajustez la hauteur des éléments de l'interface",
        );
        ui.add_space(8.0);

        let mut control_height = self.preferences.control_height();
        let response = ui.add(Slider::new(&mut control_height, 20.0..=48.0).text("Hauteur"));
        if response.changed() {
            self.preferences.set_control_height(control_height);
            self.pending_control_height = Some(control_height);
            self.mark_changed();
        }
    }

    // ========================
    // Accessibilité (Accessibility) section
    // ========================
    fn accessibility_content(&mut self, ui: &mut Ui) {
        ui.heading(RichText::new("Accessibilité").color(self.theme.palette.text_primary));
        ui.label(
            RichText::new("Options d'accessibilité et de mouvement")
                .color(self.theme.palette.text_secondary),
        );
        ui.add_space(16.0);

        // Motion subsection
        self.section_header(
            ui,
            "Animations",
            "Contrôlez l'intensité et les préférences d'animation",
        );
        ui.add_space(8.0);

        let icon_id = ui.make_persistent_id("prefs-motion-intensity-icon");
        ComboBox::from_label(
            RichText::new("Intensité des animations").color(self.theme.palette.text_secondary),
        )
        .selected_text(motion_intensity_label(self.preferences.motion_intensity()))
        .icon(menu::combo_icon(Arc::clone(&self.theme), icon_id))
        .show_ui(ui, |ui| {
            menu::with_menu_popup_motion(ui, "prefs-motion-intensity-menu", |ui| {
                let current_intensity = self.preferences.motion_intensity();
                let mut selected_intensity = current_intensity;
                for intensity in [
                    MotionIntensity::Low,
                    MotionIntensity::Medium,
                    MotionIntensity::High,
                ] {
                    if menu::menu_item(
                        ui,
                        &self.theme,
                        (
                            "prefs-motion-intensity-item",
                            motion_intensity_label(intensity),
                        ),
                        motion_intensity_label(intensity),
                        selected_intensity == intensity,
                    )
                    .clicked()
                    {
                        selected_intensity = intensity;
                    }
                }
                if selected_intensity != current_intensity {
                    self.preferences.set_motion_intensity(selected_intensity);
                    self.pending_preferences = Some(self.preferences.clone());
                }
            });
        });

        ui.add_space(12.0);

        let mut reduced_motion = self.preferences.reduced_motion();
        let response = ui.checkbox(&mut reduced_motion, "Réduire les animations");
        if response.changed() {
            self.preferences.set_reduced_motion(reduced_motion);
            self.mark_changed();
        }

        ui.add_space(8.0);

        let mut performance_mode = self.preferences.performance_mode();
        let response = ui.checkbox(&mut performance_mode, "Mode performance")
            .on_hover_text(
                "Réduit les effets d'animation pour maintenir l'interface réactive sur du matériel moins puissant.",
            );
        if response.changed() {
            self.preferences.set_performance_mode(performance_mode);
            self.mark_changed();
        }
    }

    // ========================
    // GitSpace section
    // ========================
    fn gitspace_content(&mut self, ui: &mut Ui, notifications: &mut NotificationCenter) {
        ui.heading(RichText::new("GitSpace").color(self.theme.palette.text_primary));
        ui.label(
            RichText::new("Paramètres spécifiques à GitSpace")
                .color(self.theme.palette.text_secondary),
        );
        ui.add_space(16.0);

        // Repositories subsection
        self.section_header(
            ui,
            "Dépôts",
            "Contrôlez les paramètres par défaut pour les clones",
        );
        ui.add_space(8.0);

        let control_height = ui.spacing().interact_size.y;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Destination par défaut").color(self.theme.palette.text_secondary),
            );
            ui.add_sized(
                [280.0, control_height],
                TextEdit::singleline(self.preferences.default_clone_path_mut())
                    .hint_text("/home/me/code"),
            );

            if ui.button("Choisir").clicked() {
                if let Some(path) = FileDialog::new().pick_folder() {
                    self.preferences
                        .set_default_clone_path(path.display().to_string());
                    self.pending_preferences = Some(self.preferences.clone());
                }
            }

            if ui.button("Choisir (natif)").clicked() {
                let request = DialogOpenRequest {
                    kind: "open_folder".to_string(),
                    title: Some("Sélectionner la destination par défaut".to_string()),
                    filters: Vec::new(),
                    options: DialogOptions {
                        multi_select: false,
                        show_hidden: false,
                    },
                };
                match DotnetClient::helper().dialog_open(request) {
                    Ok(response) => {
                        if response.cancelled || response.selected_paths.is_empty() {
                            self.native_dialog_status = Some("Dialogue natif annulé.".to_string());
                        } else {
                            let selected = &response.selected_paths[0];
                            self.preferences.set_default_clone_path(selected.clone());
                            self.pending_preferences = Some(self.preferences.clone());
                            self.native_dialog_status = Some(format!("Sélectionné: {}", selected));
                        }
                    }
                    Err(err) => {
                        notifications.push(Notification::error(
                            "Helper natif échoué",
                            err.user_message(),
                        ));
                        self.native_dialog_status = Some(format!("Helper natif échoué: {}", err));
                    }
                }
            }
        });

        if let Some(status) = &self.native_dialog_status {
            ui.add_space(4.0);
            ui.label(RichText::new(status).color(self.theme.palette.text_secondary));
        }

        ui.add_space(20.0);

        // Network subsection
        self.section_header(ui, "Réseau", "Configurez les paramètres réseau et proxy");
        ui.add_space(8.0);

        let network = self.preferences.network_mut();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Proxy HTTP").color(self.theme.palette.text_secondary));
            ui.add_sized(
                [200.0, control_height],
                TextEdit::singleline(&mut network.http_proxy).hint_text("http://proxy:8080"),
            );
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Proxy HTTPS").color(self.theme.palette.text_secondary));
            ui.add_sized(
                [200.0, control_height],
                TextEdit::singleline(&mut network.https_proxy).hint_text("https://proxy:8443"),
            );
        });

        ui.add_space(4.0);
        let mut timeout_error = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Timeout (sec)").color(self.theme.palette.text_secondary));
            let mut timeout_str = network.network_timeout_secs.to_string();
            let response = ui.add_sized(
                [90.0, control_height],
                TextEdit::singleline(&mut timeout_str),
            );
            if response.changed() {
                match timeout_str.trim().parse::<u64>() {
                    Ok(parsed) => {
                        network.network_timeout_secs = parsed;
                    }
                    Err(_) => {
                        timeout_error = Some("Veuillez entrer un nombre valide".to_string());
                    }
                }
            }
        });
        if let Some(error) = timeout_error {
            ui.colored_label(self.theme.palette.accent, error);
        }

        ui.add_space(4.0);
        let prev_use_https = network.use_https;
        let prev_allow_ssh = network.allow_ssh;
        ui.horizontal(|ui| {
            ui.checkbox(&mut network.use_https, "Préférer HTTPS");
            ui.checkbox(&mut network.allow_ssh, "Autoriser SSH");
        });
        if network.use_https != prev_use_https || network.allow_ssh != prev_allow_ssh {
            self.pending_preferences = Some(self.preferences.clone());
        }

        ui.add_space(8.0);
        let mut auto_fetch_enabled = self.preferences.auto_fetch_enabled();
        let response = ui.checkbox(
            &mut auto_fetch_enabled,
            "Récupération automatique des remotes",
        );
        if response.changed() {
            self.preferences.set_auto_fetch_enabled(auto_fetch_enabled);
            self.pending_preferences = Some(self.preferences.clone());
        }

        if auto_fetch_enabled {
            ui.add_space(4.0);
            ui.add_enabled_ui(true, |ui| {
                let icon_id = ui.make_persistent_id("prefs-auto-fetch-interval-icon");
                let preset_intervals = [1_u64, 5, 15];
                let current_interval = self.preferences.auto_fetch_interval_minutes();
                let selected_text = if preset_intervals.contains(&current_interval) {
                    auto_fetch_interval_label(current_interval)
                } else {
                    "Personnalisé".to_string()
                };
                ComboBox::from_label(
                    RichText::new("Intervalle de récupération")
                        .color(self.theme.palette.text_secondary),
                )
                .selected_text(selected_text)
                .icon(menu::combo_icon(Arc::clone(&self.theme), icon_id))
                .show_ui(ui, |ui| {
                    menu::with_menu_popup_motion(ui, "prefs-auto-fetch-interval-menu", |ui| {
                        let mut selected_interval = current_interval;
                        for interval in preset_intervals {
                            let label = auto_fetch_interval_label(interval);
                            if menu::menu_item(
                                ui,
                                &self.theme,
                                ("prefs-auto-fetch-interval-item", label.as_str()),
                                label.as_str(),
                                selected_interval == interval,
                            )
                            .clicked()
                            {
                                selected_interval = interval;
                            }
                        }
                        if menu::menu_item(
                            ui,
                            &self.theme,
                            ("prefs-auto-fetch-interval-item", "custom"),
                            "Personnalisé",
                            !preset_intervals.contains(&selected_interval),
                        )
                        .clicked()
                        {
                            selected_interval = current_interval;
                        }
                        if selected_interval != current_interval {
                            self.preferences
                                .set_auto_fetch_interval_minutes(selected_interval);
                            self.pending_preferences = Some(self.preferences.clone());
                        }
                    });
                });

                ui.add_space(4.0);
                let mut interval_minutes = current_interval.to_string();
                let mut interval_error = None;
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Intervalle (min)").color(self.theme.palette.text_secondary),
                    );
                    let response = ui.add_sized(
                        [90.0, control_height],
                        TextEdit::singleline(&mut interval_minutes),
                    );
                    if response.changed() {
                        match interval_minutes.trim().parse::<u64>() {
                            Ok(value) if value > 0 => {
                                self.preferences.set_auto_fetch_interval_minutes(value);
                            }
                            Ok(_) => {
                                interval_error =
                                    Some("L'intervalle doit être d'au moins 1 minute.".to_string());
                            }
                            Err(_) => {
                                interval_error =
                                    Some("Entrez un nombre entier de minutes.".to_string());
                            }
                        }
                    }
                });
                if let Some(error) = interval_error {
                    ui.colored_label(self.theme.palette.accent, error);
                }
            });
        }

        ui.add_space(20.0);

        // Keybindings subsection
        self.section_header(
            ui,
            "Raccourcis clavier",
            "Associez vos raccourcis favoris aux actions fréquentes",
        );
        ui.add_space(8.0);

        let mut remove_index: Option<usize> = None;
        for (idx, binding) in self.preferences.keybindings_mut().iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [180.0, control_height],
                    TextEdit::singleline(&mut binding.action).hint_text("Action"),
                );
                ui.add_sized(
                    [140.0, control_height],
                    TextEdit::singleline(&mut binding.binding).hint_text("Raccourci"),
                );
                if ui.button("Supprimer").clicked() {
                    remove_index = Some(idx);
                }
            });
            ui.add_space(4.0);
        }

        if let Some(index) = remove_index {
            self.preferences.keybindings_mut().remove(index);
            self.pending_preferences = Some(self.preferences.clone());
        }

        if ui.button("Ajouter un raccourci").clicked() {
            self.preferences
                .keybindings_mut()
                .push(Keybinding::default());
            self.pending_preferences = Some(self.preferences.clone());
        }
    }

    fn section_header(&self, ui: &mut Ui, title: &str, subtitle: &str) {
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            let rect = ui.available_rect_before_wrap();
            ui.painter().rect_filled(
                Rect::from_min_size(rect.min, Vec2::new(3.0, 40.0)),
                Rounding::ZERO,
                self.theme.palette.accent,
            );
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(title)
                        .color(self.theme.palette.text_primary)
                        .strong()
                        .size(15.0),
                );
                ui.label(
                    RichText::new(subtitle)
                        .color(self.theme.palette.text_secondary)
                        .size(12.0),
                );
            });
        });
    }
}

fn mode_label(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Latte => "Latte",
        ThemeMode::Frappe => "Frappe",
        ThemeMode::Macchiato => "Macchiato",
        ThemeMode::Mocha => "Mocha",
    }
}

fn motion_intensity_label(intensity: MotionIntensity) -> &'static str {
    match intensity {
        MotionIntensity::Low => "Faible",
        MotionIntensity::Medium => "Moyenne",
        MotionIntensity::High => "Élevée",
    }
}

fn auto_fetch_interval_label(minutes: u64) -> String {
    format!("{minutes} min")
}
