use std::sync::Arc;

use eframe::egui::{collapsing_header::CollapsingState, ComboBox, RichText, Slider, TextEdit, Ui};
use rfd::FileDialog;

use crate::config::{
    Keybinding, LoggingOptions, MotionIntensity, Preferences, ThemeMode, MAX_LOG_RETENTION_FILES,
    MIN_LOG_RETENTION_FILES,
};
use crate::ui::menu;
use crate::ui::theme::SharedTheme;

pub struct SettingsPanel {
    theme: SharedTheme,
    preferences: Preferences,
    logging: LoggingOptions,
    pending_preferences: Option<Preferences>,
    pending_logging: Option<LoggingOptions>,
    pending_control_height: Option<f32>,
    import_status: Option<String>,
    export_status: Option<String>,
}

impl SettingsPanel {
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
        }
    }

    pub fn set_theme(&mut self, theme: SharedTheme) {
        self.theme = theme;
    }

    pub fn set_preferences(&mut self, preferences: Preferences) {
        self.preferences = preferences;
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

    pub fn ui(&mut self, ui: &mut Ui) {
        ui.add_space(8.0);
        ui.heading(RichText::new("Settings").color(self.theme.palette.text_primary));
        ui.label(
            RichText::new(
                "Customize GitSpace to your liking. Adjust theme, shortcuts, clone destinations, and network behavior.",
            )
            .color(self.theme.palette.text_secondary),
        );
        ui.add_space(12.0);

        self.theme_section(ui);
        self.clone_section(ui);
        self.keybinding_section(ui);
        self.network_section(ui);
        self.logging_section(ui);
        self.privacy_section(ui);
        self.motion_section(ui);
        ui.add_space(4.0);
        self.actions(ui);
        ui.add_space(10.0);
        self.import_export(ui);
    }

    fn theme_section(&mut self, ui: &mut Ui) {
        self.collapsible_section(
            ui,
            "settings-appearance",
            "Appearance",
            "Choose a Catppuccin flavor for the GitSpace UI.",
            |ui, panel| {
                let icon_id = ui.make_persistent_id("settings-theme-icon");
                ComboBox::from_label(
                    RichText::new("Theme").color(panel.theme.palette.text_secondary),
                )
                .selected_text(mode_label(panel.preferences.theme_mode()))
                .icon(menu::combo_icon(Arc::clone(&panel.theme), icon_id))
                .show_ui(ui, |ui| {
                    menu::with_menu_popup_motion(ui, "settings-theme-menu", |ui| {
                        let mut selected_mode = panel.preferences.theme_mode();
                        for mode in [
                            ThemeMode::Latte,
                            ThemeMode::Frappe,
                            ThemeMode::Macchiato,
                            ThemeMode::Mocha,
                        ] {
                            if menu::menu_item(
                                ui,
                                &panel.theme,
                                ("settings-theme-item", mode_label(mode)),
                                mode_label(mode),
                                selected_mode == mode,
                            )
                            .clicked()
                            {
                                selected_mode = mode;
                            }
                        }
                        panel.preferences.set_theme_mode(selected_mode);
                    });
                });

                ui.add_space(6.0);
                let mut control_height = panel.preferences.control_height();
                let response =
                    ui.add(Slider::new(&mut control_height, 20.0..=48.0).text("Control height"));
                if response.changed() {
                    panel.preferences.set_control_height(control_height);
                    panel.pending_control_height = Some(control_height);
                }
            },
        );
    }

    fn clone_section(&mut self, ui: &mut Ui) {
        self.collapsible_section(
            ui,
            "settings-repositories",
            "Repositories",
            "Control defaults for new clones.",
            |ui, panel| {
                let control_height = ui.spacing().interact_size.y;
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Default destination")
                            .color(panel.theme.palette.text_secondary),
                    );
                    ui.add_sized(
                        [340.0, control_height],
                        TextEdit::singleline(panel.preferences.default_clone_path_mut())
                            .hint_text("/home/me/code"),
                    );

                    if ui.button("Choose folder").clicked() {
                        if let Some(path) = FileDialog::new().pick_folder() {
                            panel
                                .preferences
                                .set_default_clone_path(path.display().to_string());
                        }
                    }
                });
            },
        );
    }

    fn keybinding_section(&mut self, ui: &mut Ui) {
        self.collapsible_section(
            ui,
            "settings-keybindings",
            "Keybindings",
            "Map your favorite shortcuts to frequent actions.",
            |ui, panel| {
                let control_height = ui.spacing().interact_size.y;
                let mut remove_index: Option<usize> = None;
                for (idx, binding) in panel.preferences.keybindings_mut().iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [200.0, control_height],
                            TextEdit::singleline(&mut binding.action).hint_text("Action"),
                        );
                        ui.add_sized(
                            [160.0, control_height],
                            TextEdit::singleline(&mut binding.binding).hint_text("Shortcut"),
                        );
                        if ui.button("Remove").clicked() {
                            remove_index = Some(idx);
                        }
                    });
                    ui.add_space(4.0);
                }

                if let Some(index) = remove_index {
                    panel.preferences.keybindings_mut().remove(index);
                }

                if ui.button("Add keybinding").clicked() {
                    panel
                        .preferences
                        .keybindings_mut()
                        .push(Keybinding::default());
                }
            },
        );
    }

    fn network_section(&mut self, ui: &mut Ui) {
        self.collapsible_section(
            ui,
            "settings-network",
            "Network",
            "Control how GitSpace connects to providers and proxies.",
            |ui, panel| {
                let control_height = ui.spacing().interact_size.y;
                let network = panel.preferences.network_mut();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("HTTP proxy").color(panel.theme.palette.text_secondary));
                    ui.add_sized(
                        [200.0, control_height],
                        TextEdit::singleline(&mut network.http_proxy)
                            .hint_text("http://proxy:8080"),
                    );
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("HTTPS proxy").color(panel.theme.palette.text_secondary),
                    );
                    ui.add_sized(
                        [200.0, control_height],
                        TextEdit::singleline(&mut network.https_proxy)
                            .hint_text("https://proxy:8443"),
                    );
                });

                ui.add_space(4.0);
                let mut timeout_error = None;
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Timeout (sec)").color(panel.theme.palette.text_secondary),
                    );
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
                                timeout_error =
                                    Some("Veuillez entrer un nombre valide".to_string());
                            }
                        }
                    }
                });
                if let Some(error) = timeout_error {
                    ui.colored_label(panel.theme.palette.accent, error);
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut network.use_https, "Prefer HTTPS");
                    ui.checkbox(&mut network.allow_ssh, "Allow SSH");
                });

                ui.add_space(8.0);
                let mut auto_fetch_enabled = panel.preferences.auto_fetch_enabled();
                ui.checkbox(&mut auto_fetch_enabled, "Auto-fetch remotes");
                panel.preferences.set_auto_fetch_enabled(auto_fetch_enabled);

                ui.add_space(4.0);
                ui.add_enabled_ui(auto_fetch_enabled, |ui| {
                    let icon_id = ui.make_persistent_id("settings-auto-fetch-interval-icon");
                    let preset_intervals = [1_u64, 5, 15];
                    let current_interval = panel.preferences.auto_fetch_interval_minutes();
                    let selected_text = if preset_intervals.contains(&current_interval) {
                        auto_fetch_interval_label(current_interval)
                    } else {
                        "Custom".to_string()
                    };
                    ComboBox::from_label(
                        RichText::new("Auto-fetch interval")
                            .color(panel.theme.palette.text_secondary),
                    )
                    .selected_text(selected_text)
                    .icon(menu::combo_icon(Arc::clone(&panel.theme), icon_id))
                    .show_ui(ui, |ui| {
                        menu::with_menu_popup_motion(
                            ui,
                            "settings-auto-fetch-interval-menu",
                            |ui| {
                                let mut selected_interval = current_interval;
                                for interval in preset_intervals {
                                    let label = auto_fetch_interval_label(interval);
                                    if menu::menu_item(
                                        ui,
                                        &panel.theme,
                                        ("settings-auto-fetch-interval-item", label.as_str()),
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
                                    &panel.theme,
                                    ("settings-auto-fetch-interval-item", "custom"),
                                    "Custom",
                                    !preset_intervals.contains(&selected_interval),
                                )
                                .clicked()
                                {
                                    selected_interval = current_interval;
                                }
                                panel
                                    .preferences
                                    .set_auto_fetch_interval_minutes(selected_interval);
                            },
                        );
                    });

                    ui.add_space(4.0);
                    let mut interval_minutes = current_interval.to_string();
                    let mut interval_error = None;
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Interval (min)")
                                .color(panel.theme.palette.text_secondary),
                        );
                        let response = ui.add_sized(
                            [90.0, control_height],
                            TextEdit::singleline(&mut interval_minutes),
                        );
                        if response.changed() {
                            match interval_minutes.trim().parse::<u64>() {
                                Ok(value) if value > 0 => {
                                    panel.preferences.set_auto_fetch_interval_minutes(value);
                                }
                                Ok(_) => {
                                    interval_error =
                                        Some("Interval must be at least 1 minute.".to_string());
                                }
                                Err(_) => {
                                    interval_error =
                                        Some("Enter a whole number of minutes.".to_string());
                                }
                            }
                        }
                    });
                    if let Some(error) = interval_error {
                        ui.colored_label(panel.theme.palette.accent, error);
                    }
                });
            },
        );
    }

    fn privacy_section(&mut self, ui: &mut Ui) {
        self.collapsible_section(
            ui,
            "settings-privacy",
            "Privacy",
            "Control token storage and security settings.",
            |ui, panel| {
                let mut encrypted_tokens = panel.preferences.allow_encrypted_tokens();
                ui.checkbox(
                    &mut encrypted_tokens,
                    "Allow encrypted file storage if the native keyring is unavailable",
                )
                .on_hover_text(
                    "GitSpace uses the OS keyring by default. Enable this to fall back to a locally encrypted file when keyring access fails.",
                );
                panel
                    .preferences
                    .set_allow_encrypted_tokens(encrypted_tokens);
            },
        );
    }

    fn logging_section(&mut self, ui: &mut Ui) {
        self.collapsible_section(
            ui,
            "settings-logging",
            "Logging",
            "Configure how many log files GitSpace keeps on disk.",
            |ui, panel| {
                let mut retention_files = panel.logging.retention_files() as u32;
                let response = ui.add(
                    Slider::new(
                        &mut retention_files,
                        MIN_LOG_RETENTION_FILES as u32..=MAX_LOG_RETENTION_FILES as u32,
                    )
                    .text("Retained log files"),
                );
                if response.changed() {
                    panel.logging.set_retention_files(retention_files as usize);
                }
            },
        );
    }

    fn motion_section(&mut self, ui: &mut Ui) {
        self.collapsible_section(
            ui,
            "settings-motion",
            "Motion",
            "Control animation timing, intensity, and accessibility preferences.",
            |ui, panel| {
                let icon_id = ui.make_persistent_id("settings-motion-intensity-icon");
                ComboBox::from_label(
                    RichText::new("Motion intensity").color(panel.theme.palette.text_secondary),
                )
                .selected_text(motion_intensity_label(panel.preferences.motion_intensity()))
                .icon(menu::combo_icon(Arc::clone(&panel.theme), icon_id))
                .show_ui(ui, |ui| {
                    menu::with_menu_popup_motion(ui, "settings-motion-intensity-menu", |ui| {
                        let mut selected_intensity = panel.preferences.motion_intensity();
                        for intensity in [
                            MotionIntensity::Low,
                            MotionIntensity::Medium,
                            MotionIntensity::High,
                        ] {
                            if menu::menu_item(
                                ui,
                                &panel.theme,
                                (
                                    "settings-motion-intensity-item",
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
                        panel.preferences.set_motion_intensity(selected_intensity);
                    });
                });

                ui.add_space(6.0);
                let mut reduced_motion = panel.preferences.reduced_motion();
                ui.checkbox(&mut reduced_motion, "Reduce motion");
                panel.preferences.set_reduced_motion(reduced_motion);

                ui.add_space(4.0);
                let mut performance_mode = panel.preferences.performance_mode();
                ui.checkbox(&mut performance_mode, "Performance mode")
                    .on_hover_text(
                        "Reduce animation effects to keep the UI responsive on low-end hardware.",
                    );
                panel.preferences.set_performance_mode(performance_mode);
            },
        );
    }

    fn actions(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("Save preferences").clicked() {
                self.pending_preferences = Some(self.preferences.clone());
                self.pending_logging = Some(self.logging);
                self.import_status = Some("Ready to apply changes".to_string());
            }

            if ui.button("Reset to defaults").clicked() {
                self.preferences = Preferences::default();
                self.logging = LoggingOptions::default();
            }
        });

        if let Some(status) = &self.import_status {
            ui.add_space(6.0);
            ui.label(RichText::new(status).color(self.theme.palette.text_secondary));
        }
    }

    fn import_export(&mut self, ui: &mut Ui) {
        self.collapsible_section(
            ui,
            "settings-import-export",
            "Import / Export",
            "Move your GitSpace preferences between machines as JSON.",
            |ui, panel| {
                ui.horizontal(|ui| {
                    if ui.button("Import settings").clicked() {
                        if let Some(path) =
                            FileDialog::new().add_filter("JSON", &["json"]).pick_file()
                        {
                            match Preferences::from_path(&path) {
                                Ok(prefs) => {
                                    panel.preferences = prefs.clone();
                                    panel.pending_preferences = Some(prefs);
                                    panel.import_status = Some(format!(
                                        "Imported preferences from {}",
                                        path.display()
                                    ));
                                }
                                Err(err) => {
                                    panel.import_status = Some(err.to_string());
                                }
                            }
                        }
                    }

                    if ui.button("Export settings").clicked() {
                        if let Some(path) = FileDialog::new()
                            .add_filter("JSON", &["json"])
                            .set_file_name("gitspace-preferences.json")
                            .save_file()
                        {
                            match panel.preferences.save_to_path(&path) {
                                Ok(_) => {
                                    panel.export_status =
                                        Some(format!("Saved preferences to {}", path.display()));
                                }
                                Err(err) => panel.export_status = Some(err.to_string()),
                            }
                        }
                    }
                });

                if let Some(status) = &panel.export_status {
                    ui.add_space(6.0);
                    ui.label(RichText::new(status).color(panel.theme.palette.text_secondary));
                }
            },
        );
    }

    fn collapsible_section(
        &mut self,
        ui: &mut Ui,
        id: &str,
        title: &str,
        subtitle: &str,
        add_contents: impl FnOnce(&mut Ui, &mut Self),
    ) {
        CollapsingState::load_with_default_open(ui.ctx(), ui.make_persistent_id(id), true)
            .show_header(ui, |ui| {
                ui.vertical(|ui| {
                    ui.heading(RichText::new(title).color(self.theme.palette.text_primary));
                    ui.label(RichText::new(subtitle).color(self.theme.palette.text_secondary));
                });
            })
            .body(|ui| {
                ui.add_space(6.0);
                add_contents(ui, self);
            });
        ui.add_space(12.0);
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
        MotionIntensity::Low => "Low",
        MotionIntensity::Medium => "Medium",
        MotionIntensity::High => "High",
    }
}

fn auto_fetch_interval_label(minutes: u64) -> String {
    format!("{minutes} min")
}
