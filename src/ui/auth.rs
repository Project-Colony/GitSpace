use eframe::egui::{self, Align, Layout, RichText, TextEdit, Ui};
use poll_promise::Promise;

use crate::auth::AuthManager;
use crate::ui::theme::Theme;

pub struct AuthPanel {
    theme: Theme,
    auth: AuthManager,
    host: String,
    token: String,
    status: Option<String>,
    validation: Option<Promise<Result<(), String>>>,
}

impl AuthPanel {
    pub fn new(theme: Theme, auth: AuthManager) -> Self {
        Self {
            theme,
            auth,
            host: "github.com".to_string(),
            token: String::new(),
            status: None,
            validation: None,
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn set_auth_manager(&mut self, auth: AuthManager) {
        self.auth = auth;
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.poll_validation();

        ui.add_space(8.0);
        ui.heading(
            RichText::new("Authentication")
                .color(self.theme.palette.text_primary)
                .strong(),
        );
        ui.label(
            RichText::new(
                "Save personal access tokens for Git providers so GitSpace can reuse them automatically.",
            )
            .color(self.theme.palette.text_secondary),
        );
        ui.add_space(10.0);

        egui::Frame::none()
            .fill(self.theme.palette.surface)
            .stroke(egui::Stroke::new(1.0, self.theme.palette.surface_highlight))
            .rounding(egui::Rounding::same(8.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("Remote host").color(self.theme.palette.text_secondary));
                    ui.add_sized(
                        [320.0, 28.0],
                        TextEdit::singleline(&mut self.host).hint_text("github.com or gitlab.com"),
                    );
                    ui.label(
                        RichText::new(
                            "Use only the host name (no repository path).\nExamples:\n• GitHub: github.com\n• GitLab: gitlab.com",
                        )
                        .color(self.theme.palette.text_secondary),
                    );

                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Access token").color(self.theme.palette.text_secondary),
                    );
                    ui.add_sized(
                        [320.0, 28.0],
                        TextEdit::singleline(&mut self.token)
                            .password(true)
                            .hint_text("Paste your personal access token"),
                    );

                    ui.add_space(10.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let button = ui.add_enabled(
                            !self.host.trim().is_empty() && !self.token.trim().is_empty(),
                            egui::Button::new("Validate & Save"),
                        );
                        if button.clicked() {
                            self.start_validation();
                        }
                    });

                    if let Some(status) = &self.status {
                        ui.add_space(6.0);
                        ui.colored_label(self.theme.palette.text_secondary, status);
                    }
                });
                ui.add_space(8.0);
            });

        ui.add_space(12.0);
        ui.heading(RichText::new("Saved hosts").color(self.theme.palette.text_primary));
        let hosts = self.auth.known_hosts();
        if hosts.is_empty() {
            ui.label(
                RichText::new("No saved tokens yet. Add a host above to store a credential.")
                    .color(self.theme.palette.text_secondary),
            );
        } else {
            for host in hosts {
                ui.horizontal(|ui| {
                    ui.colored_label(self.theme.palette.text_primary, host.clone());
                    if ui
                        .add(egui::Button::new("Remove").fill(self.theme.palette.surface_highlight))
                        .clicked()
                    {
                        let _ = self.auth.clear_token(&host);
                        self.status = Some(format!("Removed token for {}", host));
                    }
                });
            }
        }
    }

    fn start_validation(&mut self) {
        let host = self.host.trim().to_string();
        let token = self.token.trim().to_string();
        let auth = self.auth.clone();
        self.status = Some("Validating token...".to_string());
        self.validation = Some(Promise::spawn_thread("validate_token", move || {
            auth.validate_and_store(&host, &token)
        }));
    }

    fn poll_validation(&mut self) {
        if let Some(promise) = &self.validation {
            if let Some(result) = promise.ready() {
                let result = result.clone();
                self.validation = None;
                match result {
                    Ok(_) => {
                        self.status = Some("Token validated and saved.".to_string());
                        self.token.clear();
                    }
                    Err(err) => self.status = Some(format!("Validation failed: {}", err)),
                }
            }
        }
    }
}
