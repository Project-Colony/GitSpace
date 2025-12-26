use eframe::egui::{self, RichText, TextEdit, Ui};
use poll_promise::Promise;

use crate::auth::AuthManager;
use crate::ui::theme::Theme;

pub struct AuthPanel {
    theme: Theme,
    auth: AuthManager,
    github_host: String,
    github_token: String,
    github_status: Option<String>,
    github_validation: Option<Promise<Result<(), String>>>,
    gitlab_host: String,
    gitlab_token: String,
    gitlab_status: Option<String>,
    gitlab_validation: Option<Promise<Result<(), String>>>,
}

impl AuthPanel {
    pub fn new(theme: Theme, auth: AuthManager) -> Self {
        Self {
            theme,
            auth,
            github_host: "github.com".to_string(),
            github_token: String::new(),
            github_status: None,
            github_validation: None,
            gitlab_host: "gitlab.com".to_string(),
            gitlab_token: String::new(),
            gitlab_status: None,
            gitlab_validation: None,
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn set_auth_manager(&mut self, auth: AuthManager) {
        self.auth = auth;
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        poll_validation(
            &mut self.github_validation,
            &mut self.github_status,
            &mut self.github_token,
        );
        poll_validation(
            &mut self.gitlab_validation,
            &mut self.gitlab_status,
            &mut self.gitlab_token,
        );

        let layout = AuthLayout::new(&self.theme);
        ui.add_space(layout.spacing.md);
        layout.header(
            ui,
            "Authentication",
            "Save personal access tokens for Git providers so GitSpace can reuse them automatically.",
        );

        layout.section(ui, AuthSection::provider("GitHub", '\u{f408}'), |ui| {
            provider_section(
                ui,
                &layout,
                &self.auth,
                &mut self.github_host,
                &mut self.github_token,
                &mut self.github_status,
                &mut self.github_validation,
                "github.com",
            );
        });

        ui.add_space(layout.spacing.sm);
        layout.section(ui, AuthSection::provider("GitLab", '\u{f296}'), |ui| {
            provider_section(
                ui,
                &layout,
                &self.auth,
                &mut self.gitlab_host,
                &mut self.gitlab_token,
                &mut self.gitlab_status,
                &mut self.gitlab_validation,
                "gitlab.com",
            );
        });

        ui.add_space(layout.spacing.lg);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                layout.section(
                    ui,
                    AuthSection::info("Examples", "Host only, no repository path needed."),
                    |ui| {
                        ui.label(
                            RichText::new(
                                "Remote host\ngithub.com\nExample: github.com/MotherSphere",
                            )
                            .color(layout.theme.palette.text_secondary),
                        );
                        ui.add_space(layout.spacing.sm);
                        ui.label(
                            RichText::new(
                                "Remote host\ngitlab.com\nExample: gitlab.com/MotherSphere",
                            )
                            .color(layout.theme.palette.text_secondary),
                        );
                    },
                );
            });
            ui.add_space(layout.spacing.lg);
            ui.vertical(|ui| {
                layout.section(
                    ui,
                    AuthSection::info("Saved hosts", "Stored credentials available to GitSpace."),
                    |ui| {
                        let hosts = self.auth.known_hosts();
                        if hosts.is_empty() {
                            ui.label(
                                RichText::new(
                                    "No saved tokens yet. Add a host above to store a credential.",
                                )
                                .color(layout.theme.palette.text_secondary),
                            );
                        } else {
                            for host in hosts {
                                ui.horizontal(|ui| {
                                    ui.colored_label(
                                        layout.theme.palette.text_primary,
                                        host.clone(),
                                    );
                                    let remove_button = AuthActionButton::new("Remove")
                                        .variant(ActionVariant::Secondary)
                                        .small();
                                    if remove_button.show(ui, layout.theme).clicked() {
                                        let _ = self.auth.clear_token(&host);
                                        let message = Some(format!("Removed token for {}", host));
                                        self.github_status = message.clone();
                                        self.gitlab_status = message;
                                    }
                                });
                                ui.add_space(layout.spacing.xs);
                            }
                        }
                    },
                );
            });
        });
    }
}

fn provider_section(
    ui: &mut Ui,
    layout: &AuthLayout<'_>,
    auth: &AuthManager,
    host: &mut String,
    token: &mut String,
    status: &mut Option<String>,
    validation: &mut Option<Promise<Result<(), String>>>,
    host_hint: &str,
) {
    let control_height = ui.spacing().interact_size.y;
    AuthTextField::new("Remote host", host)
        .hint_text(host_hint)
        .width(layout.metrics.host_width)
        .show(ui, layout.theme, control_height);
    ui.add_space(layout.spacing.sm);
    ui.label(RichText::new("Access token").color(layout.theme.palette.text_secondary));
    ui.horizontal(|ui| {
        let edit = TextEdit::singleline(token)
            .hint_text("Paste your personal access token")
            .password(true);
        ui.add_sized([layout.metrics.token_width, control_height], edit);
        ui.add_space(layout.spacing.sm);
        let enabled = !host.trim().is_empty() && !token.trim().is_empty();
        let button = AuthActionButton::new("Validate & Save")
            .variant(ActionVariant::Primary)
            .show_enabled(ui, layout.theme, enabled);
        if button.clicked() {
            start_validation(auth, host, token, status, validation);
        }
    });
    ui.add_space(layout.spacing.md);

    if let Some(current_status) = status {
        ui.add_space(layout.spacing.sm);
        ui.colored_label(layout.theme.palette.text_secondary, current_status);
    }
}

fn start_validation(
    auth: &AuthManager,
    host: &str,
    token: &str,
    status: &mut Option<String>,
    validation: &mut Option<Promise<Result<(), String>>>,
) {
    let host = host.trim().to_string();
    let token = token.trim().to_string();
    let auth = auth.clone();
    *status = Some("Validating token...".to_string());
    *validation = Some(Promise::spawn_thread("validate_token", move || {
        auth.validate_and_store(&host, &token)
    }));
}

fn poll_validation(
    validation: &mut Option<Promise<Result<(), String>>>,
    status: &mut Option<String>,
    token: &mut String,
) {
    if let Some(promise) = validation {
        if let Some(result) = promise.ready() {
            let result = result.clone();
            *validation = None;
            match result {
                Ok(_) => {
                    *status = Some("Token validated and saved.".to_string());
                    token.clear();
                }
                Err(err) => *status = Some(format!("Validation failed: {}", err)),
            }
        }
    }
}

struct AuthLayout<'a> {
    theme: &'a Theme,
    spacing: crate::ui::theme::Spacing,
    metrics: AuthLayoutMetrics,
}

impl<'a> AuthLayout<'a> {
    fn new(theme: &'a Theme) -> Self {
        let metrics = AuthLayoutMetrics::default();
        Self {
            theme,
            spacing: theme.spacing,
            metrics,
        }
    }

    fn header(&self, ui: &mut Ui, title: &str, subtitle: &str) {
        ui.heading(
            RichText::new(title)
                .color(self.theme.palette.text_primary)
                .strong(),
        );
        ui.add_space(self.spacing.xs);
        ui.label(RichText::new(subtitle).color(self.theme.palette.text_secondary));
        ui.add_space(self.spacing.lg);
    }

    fn section<F>(&self, ui: &mut Ui, section: AuthSection<'_>, content: F)
    where
        F: FnOnce(&mut Ui),
    {
        let frame = egui::Frame::none()
            .fill(self.theme.palette.surface)
            .stroke(egui::Stroke::new(1.0, self.theme.palette.surface_highlight))
            .inner_margin(egui::Margin::same(self.spacing.md))
            .rounding(egui::Rounding::same(self.spacing.xs));
        frame.show(ui, |ui| {
            section.header(ui, self.theme, self.spacing);
            ui.add_space(self.spacing.md);
            content(ui);
        });
    }
}

#[derive(Debug, Clone, Copy)]
struct AuthLayoutMetrics {
    host_width: f32,
    token_width: f32,
}

impl Default for AuthLayoutMetrics {
    fn default() -> Self {
        Self {
            host_width: 320.0,
            token_width: 360.0,
        }
    }
}

enum SectionTone {
    Provider,
    Info,
}

struct AuthSection<'a> {
    title: &'a str,
    subtitle: Option<&'a str>,
    icon: Option<char>,
    tone: SectionTone,
}

impl<'a> AuthSection<'a> {
    fn provider(title: &'a str, icon: char) -> Self {
        Self {
            title,
            subtitle: Some("Connect a token to enable Git operations."),
            icon: Some(icon),
            tone: SectionTone::Provider,
        }
    }

    fn info(title: &'a str, subtitle: &'a str) -> Self {
        Self {
            title,
            subtitle: Some(subtitle),
            icon: None,
            tone: SectionTone::Info,
        }
    }

    fn header(&self, ui: &mut Ui, theme: &Theme, spacing: crate::ui::theme::Spacing) {
        let title_text = match self.icon {
            Some(icon) => format!("{icon} {}", self.title),
            None => self.title.to_string(),
        };
        let title = match self.tone {
            SectionTone::Provider => RichText::new(title_text)
                .color(theme.palette.text_primary)
                .strong(),
            SectionTone::Info => RichText::new(title_text).color(theme.palette.text_primary),
        };
        ui.label(title);
        if let Some(subtitle) = self.subtitle {
            ui.add_space(spacing.xs);
            ui.label(RichText::new(subtitle).color(theme.palette.text_secondary));
        }
    }
}

struct AuthTextField<'a> {
    label: &'a str,
    value: &'a mut String,
    hint: &'a str,
    width: f32,
}

impl<'a> AuthTextField<'a> {
    fn new(label: &'a str, value: &'a mut String) -> Self {
        Self {
            label,
            value,
            hint: "",
            width: 300.0,
        }
    }

    fn hint_text(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }

    fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    fn show(self, ui: &mut Ui, theme: &Theme, height: f32) {
        ui.label(RichText::new(self.label).color(theme.palette.text_secondary));
        let edit = TextEdit::singleline(self.value).hint_text(self.hint);
        ui.add_sized([self.width, height], edit);
    }
}

enum ActionVariant {
    Primary,
    Secondary,
}

struct AuthActionButton<'a> {
    label: &'a str,
    variant: ActionVariant,
    small: bool,
}

impl<'a> AuthActionButton<'a> {
    fn new(label: &'a str) -> Self {
        Self {
            label,
            variant: ActionVariant::Primary,
            small: false,
        }
    }

    fn variant(mut self, variant: ActionVariant) -> Self {
        self.variant = variant;
        self
    }

    fn small(mut self) -> Self {
        self.small = true;
        self
    }

    fn show(self, ui: &mut Ui, theme: &Theme) -> egui::Response {
        self.show_enabled(ui, theme, true)
    }

    fn show_enabled(self, ui: &mut Ui, theme: &Theme, enabled: bool) -> egui::Response {
        let (fill, text_color) = match self.variant {
            ActionVariant::Primary => (theme.palette.accent, theme.palette.background),
            ActionVariant::Secondary => {
                (theme.palette.surface_highlight, theme.palette.text_primary)
            }
        };
        let text = if self.small {
            RichText::new(self.label).color(text_color)
        } else {
            RichText::new(self.label).color(text_color).strong()
        };
        ui.add_enabled(enabled, egui::Button::new(text).fill(fill))
    }
}
