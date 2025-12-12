use eframe::egui::{ComboBox, RichText, TextEdit, Ui, collapsing_header::CollapsingState};
use rfd::FileDialog;

use crate::config::{Keybinding, Preferences, ReleaseChannel, ThemeMode};
use crate::ui::theme::Theme;

pub struct SettingsPanel {
    theme: Theme,
    preferences: Preferences,
    pending_preferences: Option<Preferences>,
    import_status: Option<String>,
    export_status: Option<String>,
    update_request: bool,
    update_status: Option<String>,
    telemetry_status: Option<String>,
    telemetry_purge_requested: bool,
}

impl SettingsPanel {
    pub fn new(theme: Theme, preferences: Preferences) -> Self {
        Self {
            theme,
            preferences,
            pending_preferences: None,
            import_status: None,
            export_status: None,
            update_request: false,
            update_status: None,
            telemetry_status: None,
            telemetry_purge_requested: false,
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn set_preferences(&mut self, preferences: Preferences) {
        self.preferences = preferences;
    }

    pub fn take_changes(&mut self) -> Option<Preferences> {
        self.pending_preferences.take()
    }

    pub fn take_update_request(&mut self) -> bool {
        if self.update_request {
            self.update_request = false;
            return true;
        }

        false
    }

    pub fn set_update_status<S: Into<String>>(&mut self, status: S) {
        self.update_status = Some(status.into());
    }

    pub fn set_telemetry_status<S: Into<String>>(&mut self, status: S) {
        self.telemetry_status = Some(status.into());
    }

    pub fn take_telemetry_purge_request(&mut self) -> bool {
        if self.telemetry_purge_requested {
            self.telemetry_purge_requested = false;
            return true;
        }

        false
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
        self.privacy_section(ui);
        self.update_section(ui);
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
            "Switch between light and dark palettes.",
            |ui, panel| {
                ComboBox::from_label(
                    RichText::new("Theme mode").color(panel.theme.palette.text_secondary),
                )
                .selected_text(mode_label(panel.preferences.theme_mode()))
                .show_ui(ui, |ui| {
                    let mut selected_mode = panel.preferences.theme_mode();
                    for mode in [ThemeMode::Dark, ThemeMode::Light] {
                        ui.selectable_value(&mut selected_mode, mode, mode_label(mode));
                    }
                    panel.preferences.set_theme_mode(selected_mode);
                });
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
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Default destination")
                            .color(panel.theme.palette.text_secondary),
                    );
                    ui.add_sized(
                        [340.0, 28.0],
                        TextEdit::singleline(panel.preferences.default_clone_path_mut())
                            .hint_text("/home/me/code"),
                    );

                    if ui.button("Choose folder").clicked()
                        && let Some(path) = FileDialog::new().pick_folder()
                    {
                        panel
                            .preferences
                            .set_default_clone_path(path.display().to_string());
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
                let mut remove_index: Option<usize> = None;
                for (idx, binding) in panel.preferences.keybindings_mut().iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [200.0, 26.0],
                            TextEdit::singleline(&mut binding.action).hint_text("Action"),
                        );
                        ui.add_sized(
                            [160.0, 26.0],
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
                let network = panel.preferences.network_mut();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("HTTP proxy").color(panel.theme.palette.text_secondary));
                    ui.add_sized(
                        [200.0, 26.0],
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
                        [200.0, 26.0],
                        TextEdit::singleline(&mut network.https_proxy)
                            .hint_text("https://proxy:8443"),
                    );
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Timeout (sec)").color(panel.theme.palette.text_secondary),
                    );
                    let mut timeout_str = network.network_timeout_secs.to_string();
                    if ui
                        .add_sized([90.0, 26.0], TextEdit::singleline(&mut timeout_str))
                        .changed()
                        && let Ok(parsed) = timeout_str.parse()
                    {
                        network.network_timeout_secs = parsed;
                    }
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut network.use_https, "Prefer HTTPS");
                    ui.checkbox(&mut network.allow_ssh, "Allow SSH");
                });
            },
        );
    }

    fn privacy_section(&mut self, ui: &mut Ui) {
        self.collapsible_section(
            ui,
            "settings-privacy",
            "Privacy",
            "Opt in to anonymized diagnostics and decide what gets shared. Nothing leaves your machine unless enabled.",
            |ui, panel| {
                let mut telemetry_enabled = panel.preferences.telemetry_enabled();
                ui.checkbox(
                    &mut telemetry_enabled,
                    "Share anonymized events (feature usage, performance)",
                );
                panel.preferences.set_telemetry_enabled(telemetry_enabled);

                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Collected: launch/session counts, tab switches, hashed repository identifiers. Excludes content or credentials.",
                    )
                    .color(panel.theme.palette.text_secondary),
                );

                ui.add_space(6.0);
                if ui.button("Purge collected diagnostics").clicked() {
                    panel.telemetry_purge_requested = true;
                    panel.telemetry_status = Some("Queued telemetry purge".to_string());
                }

                if let Some(status) = &panel.telemetry_status {
                    ui.add_space(4.0);
                    ui.label(RichText::new(status).color(panel.theme.palette.text_secondary));
                }
            },
        );
    }

    fn update_section(&mut self, ui: &mut Ui) {
        self.collapsible_section(
            ui,
            "settings-updates",
            "Updates",
            "Control how GitSpace checks for new versions and which release channel you follow.",
            |ui, panel| {
                let mut auto_check = panel.preferences.auto_check_updates();
                ui.checkbox(&mut auto_check, "Automatically check for updates on launch");
                panel.preferences.set_auto_check_updates(auto_check);

                ui.add_space(4.0);
                ComboBox::from_label(
                    RichText::new("Release channel").color(panel.theme.palette.text_secondary),
                )
                .selected_text(channel_label(panel.preferences.release_channel()))
                .show_ui(ui, |ui| {
                    let mut selected_channel = panel.preferences.release_channel();
                    for channel in [ReleaseChannel::Stable, ReleaseChannel::Preview] {
                        ui.selectable_value(&mut selected_channel, channel, channel_label(channel));
                    }
                    panel.preferences.set_release_channel(selected_channel);
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Check for updates now").clicked() {
                        panel.update_request = true;
                        panel.update_status = Some("Checking for updates...".to_string());
                    }

                    if let Some(status) = &panel.update_status {
                        ui.label(RichText::new(status).color(panel.theme.palette.text_secondary));
                    }
                });
            },
        );
    }

    fn actions(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("Save preferences").clicked() {
                self.pending_preferences = Some(self.preferences.clone());
                self.import_status = Some("Ready to apply changes".to_string());
            }

            if ui.button("Reset to defaults").clicked() {
                self.preferences = Preferences::default();
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
                    if ui.button("Import settings").clicked()
                        && let Some(path) =
                            FileDialog::new().add_filter("JSON", &["json"]).pick_file()
                    {
                        match Preferences::from_path(&path) {
                            Ok(prefs) => {
                                panel.preferences = prefs.clone();
                                panel.pending_preferences = Some(prefs);
                                panel.import_status =
                                    Some(format!("Imported preferences from {}", path.display()));
                            }
                            Err(err) => {
                                panel.import_status = Some(err.to_string());
                            }
                        }
                    }

                    if ui.button("Export settings").clicked()
                        && let Some(path) = FileDialog::new()
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
        ThemeMode::Dark => "Dark",
        ThemeMode::Light => "Light",
    }
}

fn channel_label(channel: ReleaseChannel) -> &'static str {
    match channel {
        ReleaseChannel::Stable => "Stable",
        ReleaseChannel::Preview => "Preview",
    }
}
