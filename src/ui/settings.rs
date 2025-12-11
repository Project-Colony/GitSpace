use eframe::egui::{ComboBox, RichText, TextEdit, Ui};
use rfd::FileDialog;

use crate::config::{Keybinding, Preferences, ThemeMode};
use crate::ui::theme::Theme;

pub struct SettingsPanel {
    theme: Theme,
    preferences: Preferences,
    pending_preferences: Option<Preferences>,
    import_status: Option<String>,
    export_status: Option<String>,
}

impl SettingsPanel {
    pub fn new(theme: Theme, preferences: Preferences) -> Self {
        Self {
            theme,
            preferences,
            pending_preferences: None,
            import_status: None,
            export_status: None,
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
        ui.add_space(12.0);
        self.clone_section(ui);
        ui.add_space(12.0);
        self.keybinding_section(ui);
        ui.add_space(12.0);
        self.network_section(ui);
        ui.add_space(12.0);
        self.actions(ui);
        ui.add_space(12.0);
        self.import_export(ui);
    }

    fn theme_section(&mut self, ui: &mut Ui) {
        ui.heading(RichText::new("Appearance").color(self.theme.palette.text_primary));
        ui.label(
            RichText::new("Switch between light and dark palettes.")
                .color(self.theme.palette.text_secondary),
        );
        ui.add_space(6.0);

        ComboBox::from_label(RichText::new("Theme mode").color(self.theme.palette.text_secondary))
            .selected_text(mode_label(self.preferences.theme_mode()))
            .show_ui(ui, |ui| {
                let mut selected_mode = self.preferences.theme_mode();
                for mode in [ThemeMode::Dark, ThemeMode::Light] {
                    ui.selectable_value(&mut selected_mode, mode, mode_label(mode));
                }
                self.preferences.set_theme_mode(selected_mode);
            });
    }

    fn clone_section(&mut self, ui: &mut Ui) {
        ui.heading(RichText::new("Repositories").color(self.theme.palette.text_primary));
        ui.label(
            RichText::new("Control defaults for new clones.")
                .color(self.theme.palette.text_secondary),
        );
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            ui.label(RichText::new("Default destination").color(self.theme.palette.text_secondary));
            ui.add_sized(
                [340.0, 28.0],
                TextEdit::singleline(self.preferences.default_clone_path_mut())
                    .hint_text("/home/me/code"),
            );

            if ui.button("Choose folder").clicked() {
                if let Some(path) = FileDialog::new().pick_folder() {
                    self.preferences
                        .set_default_clone_path(path.display().to_string());
                }
            }
        });
    }

    fn keybinding_section(&mut self, ui: &mut Ui) {
        ui.heading(RichText::new("Keybindings").color(self.theme.palette.text_primary));
        ui.label(
            RichText::new("Map your favorite shortcuts to frequent actions.")
                .color(self.theme.palette.text_secondary),
        );
        ui.add_space(6.0);

        let mut remove_index: Option<usize> = None;
        for (idx, binding) in self.preferences.keybindings_mut().iter_mut().enumerate() {
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
            self.preferences.keybindings_mut().remove(index);
        }

        if ui.button("Add keybinding").clicked() {
            self.preferences
                .keybindings_mut()
                .push(Keybinding::default());
        }
    }

    fn network_section(&mut self, ui: &mut Ui) {
        ui.heading(RichText::new("Network").color(self.theme.palette.text_primary));
        ui.label(
            RichText::new("Control how GitSpace connects to providers and proxies.")
                .color(self.theme.palette.text_secondary),
        );
        ui.add_space(6.0);

        let network = self.preferences.network_mut();
        ui.horizontal(|ui| {
            ui.label(RichText::new("HTTP proxy").color(self.theme.palette.text_secondary));
            ui.add_sized(
                [200.0, 26.0],
                TextEdit::singleline(&mut network.http_proxy).hint_text("http://proxy:8080"),
            );
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("HTTPS proxy").color(self.theme.palette.text_secondary));
            ui.add_sized(
                [200.0, 26.0],
                TextEdit::singleline(&mut network.https_proxy).hint_text("https://proxy:8443"),
            );
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Timeout (sec)").color(self.theme.palette.text_secondary));
            let mut timeout_str = network.network_timeout_secs.to_string();
            if ui
                .add_sized([90.0, 26.0], TextEdit::singleline(&mut timeout_str))
                .changed()
            {
                if let Ok(parsed) = timeout_str.parse() {
                    network.network_timeout_secs = parsed;
                }
            }
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.checkbox(&mut network.use_https, "Prefer HTTPS");
            ui.checkbox(&mut network.allow_ssh, "Allow SSH");
        });
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
        ui.separator();
        ui.heading(RichText::new("Import / Export").color(self.theme.palette.text_primary));
        ui.label(
            RichText::new("Move your GitSpace preferences between machines as JSON.")
                .color(self.theme.palette.text_secondary),
        );
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            if ui.button("Import settings").clicked() {
                if let Some(path) = FileDialog::new().add_filter("JSON", &["json"]).pick_file() {
                    match Preferences::from_path(&path) {
                        Ok(prefs) => {
                            self.preferences = prefs.clone();
                            self.pending_preferences = Some(prefs);
                            self.import_status =
                                Some(format!("Imported preferences from {}", path.display()));
                        }
                        Err(err) => {
                            self.import_status = Some(err.to_string());
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
                    match self.preferences.save_to_path(&path) {
                        Ok(_) => {
                            self.export_status =
                                Some(format!("Saved preferences to {}", path.display()));
                        }
                        Err(err) => self.export_status = Some(err.to_string()),
                    }
                }
            }
        });

        if let Some(status) = &self.export_status {
            ui.add_space(6.0);
            ui.label(RichText::new(status).color(self.theme.palette.text_secondary));
        }
    }
}

fn mode_label(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Dark => "Dark",
        ThemeMode::Light => "Light",
    }
}
