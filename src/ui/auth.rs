//! Authentication panel UI with OAuth and token-based login.

use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use eframe::egui::{self, RichText, TextEdit, Ui};
use poll_promise::Promise;

use crate::auth::{AuthManager, DeviceFlow, OAuthProvider, OAuthResult};
use crate::ui::theme::{SharedTheme, Theme};

/// OAuth client IDs built in at compile time. When set, they are used instead of a client ID
/// entered in the UI.
///
/// GitHub (device flow): an OAuth app at https://github.com/settings/developers with
/// "Enable Device Flow" ticked; the callback URL is not used.
/// GitLab (PKCE): an application at https://gitlab.com/-/user_settings/applications with
/// "Confidential" unticked and the redirect URI http://127.0.0.1:9876/callback.
const GITHUB_CLIENT_ID: Option<&str> = option_env!("GITSPACE_GITHUB_CLIENT_ID");
const GITLAB_CLIENT_ID: Option<&str> = option_env!("GITSPACE_GITLAB_CLIENT_ID");

/// A GitHub sign-in in progress (device flow).
enum GithubSignIn {
    /// Asking GitHub for a user code.
    Requesting(Promise<Result<Box<DeviceFlow>, String>>),
    /// Waiting for the user to enter the code on GitHub.
    Waiting {
        user_code: String,
        verification_uri: String,
        result: Promise<OAuthResult>,
        /// Dropping this sender stops the polling thread.
        cancel: Sender<()>,
    },
}

pub struct AuthPanel {
    theme: SharedTheme,
    auth: AuthManager,
    // OAuth state
    github_sign_in: Option<GithubSignIn>,
    gitlab_oauth_promise: Option<Promise<OAuthResult>>,
    github_status: Option<String>,
    gitlab_status: Option<String>,
    // Manual token entry (advanced)
    manual_host: String,
    manual_token: String,
    manual_status: Option<String>,
    manual_validation: Option<Promise<Result<(), String>>>,
    // Custom OAuth client IDs
    custom_github_client_id: String,
    custom_gitlab_client_id: String,
}

impl AuthPanel {
    pub fn new(theme: SharedTheme, auth: AuthManager) -> Self {
        Self {
            theme,
            auth,
            github_sign_in: None,
            gitlab_oauth_promise: None,
            github_status: None,
            gitlab_status: None,
            manual_host: String::new(),
            manual_token: String::new(),
            manual_status: None,
            manual_validation: None,
            custom_github_client_id: String::new(),
            custom_gitlab_client_id: String::new(),
        }
    }

    pub fn set_theme(&mut self, theme: SharedTheme) {
        self.theme = theme;
    }

    pub fn set_auth_manager(&mut self, auth: AuthManager) {
        self.auth = auth;
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        // Poll async operations
        self.poll_github_sign_in();
        self.poll_gitlab_oauth();
        self.poll_manual_validation();
        if self.github_sign_in.is_some() || self.gitlab_oauth_promise.is_some() {
            // Show the code and the result without waiting for the next input event.
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }

        // Clone theme for layout to avoid borrow conflicts
        let theme = self.theme.clone();
        let layout = AuthLayout::new(&theme);

        ui.add_space(layout.spacing.md);
        layout.header(
            ui,
            "Authentication",
            "Connect your Git provider accounts to enable seamless repository access.",
        );

        // Connected accounts section
        self.connected_accounts_section(ui, &layout);

        ui.add_space(layout.spacing.lg);

        // OAuth login buttons
        self.oauth_section(ui, &layout);

        ui.add_space(layout.spacing.lg);

        // Advanced: Manual token entry
        self.manual_entry_section(ui, &layout);
    }

    fn connected_accounts_section(&mut self, ui: &mut Ui, layout: &AuthLayout<'_>) {
        layout.section(
            ui,
            AuthSection::info("Connected Accounts", "Your authenticated Git providers"),
            |ui| {
                let hosts = self.auth.known_hosts();
                let oauth_hosts: Vec<_> = hosts
                    .iter()
                    .filter(|h| !h.starts_with("oauth:"))
                    .cloned()
                    .collect();

                if oauth_hosts.is_empty() {
                    ui.label(
                        RichText::new(
                            "No accounts connected yet. Use the buttons below to log in.",
                        )
                        .color(layout.theme.palette.text_secondary),
                    );
                } else {
                    for host in oauth_hosts {
                        ui.horizontal(|ui| {
                            // Show provider icon
                            let icon = if host.contains("github") {
                                '\u{f408}'
                            } else if host.contains("gitlab") {
                                '\u{f296}'
                            } else {
                                '\u{f1d3}'
                            };

                            let has_oauth = self.auth.has_oauth(&host);
                            let auth_type = if has_oauth { "OAuth" } else { "Token" };

                            ui.label(
                                RichText::new(format!("{} {}", icon, host))
                                    .color(layout.theme.palette.text_primary),
                            );
                            ui.label(
                                RichText::new(format!("({})", auth_type))
                                    .color(layout.theme.palette.text_secondary)
                                    .small(),
                            );

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let disconnect = AuthActionButton::new("Disconnect")
                                        .variant(ActionVariant::Secondary)
                                        .small();
                                    if disconnect.show(ui, layout.theme).clicked() {
                                        if has_oauth {
                                            let _ = self.auth.clear_oauth_token(&host);
                                        } else {
                                            let _ = self.auth.clear_token(&host);
                                        }
                                    }
                                },
                            );
                        });
                        ui.add_space(layout.spacing.xs);
                    }
                }
            },
        );
    }

    fn oauth_section(&mut self, ui: &mut Ui, layout: &AuthLayout<'_>) {
        layout.section(
            ui,
            AuthSection::provider("Login with OAuth", '\u{f084}'),
            |ui| {
                ui.label(
                    RichText::new(
                        "GitHub gives you a code to enter on github.com. GitLab opens a browser window.",
                    )
                    .color(layout.theme.palette.text_secondary),
                );
                ui.add_space(layout.spacing.md);

                ui.horizontal(|ui| {
                    // GitHub OAuth button
                    let github_enabled = self.github_sign_in.is_none()
                        && (GITHUB_CLIENT_ID.is_some()
                            || !self.custom_github_client_id.trim().is_empty());

                    let github_button = egui::Button::new(
                        RichText::new("\u{f408} Login with GitHub")
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(36, 41, 46)); // GitHub dark

                    if ui.add_enabled(github_enabled, github_button).clicked() {
                        self.start_github_oauth();
                    }

                    ui.add_space(layout.spacing.md);

                    // GitLab OAuth button
                    let gitlab_enabled = self.gitlab_oauth_promise.is_none()
                        && (GITLAB_CLIENT_ID.is_some()
                            || !self.custom_gitlab_client_id.trim().is_empty());

                    let gitlab_button = egui::Button::new(
                        RichText::new("\u{f296} Login with GitLab")
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(252, 109, 38)); // GitLab orange

                    if ui.add_enabled(gitlab_enabled, gitlab_button).clicked() {
                        self.start_gitlab_oauth();
                    }
                });

                self.github_device_code(ui, layout);

                // Status messages
                if let Some(status) = &self.github_status {
                    ui.add_space(layout.spacing.sm);
                    ui.label(RichText::new(format!("GitHub: {}", status)).color(layout.theme.palette.text_secondary));
                }
                if let Some(status) = &self.gitlab_status {
                    ui.add_space(layout.spacing.sm);
                    ui.label(RichText::new(format!("GitLab: {}", status)).color(layout.theme.palette.text_secondary));
                }

                // OAuth client ID configuration
                ui.add_space(layout.spacing.lg);
                ui.collapsing("Configure OAuth Apps", |ui| {
                    ui.label(
                        RichText::new(
                            "To use OAuth, register an OAuth app with your provider and enter its Client ID.\n\
                             GitHub: tick \"Enable Device Flow\" in the app settings; the callback URL is not used.\n\
                             GitLab: untick \"Confidential\" and set the redirect URI to http://127.0.0.1:9876/callback"
                        )
                        .color(layout.theme.palette.text_secondary)
                        .small(),
                    );
                    ui.add_space(layout.spacing.sm);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("GitHub Client ID:").color(layout.theme.palette.text_secondary));
                        ui.add(
                            TextEdit::singleline(&mut self.custom_github_client_id)
                                .hint_text("Your GitHub OAuth App Client ID")
                                .desired_width(280.0),
                        );
                    });

                    ui.add_space(layout.spacing.xs);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("GitLab Client ID:").color(layout.theme.palette.text_secondary));
                        ui.add(
                            TextEdit::singleline(&mut self.custom_gitlab_client_id)
                                .hint_text("Your GitLab Application ID")
                                .desired_width(280.0),
                        );
                    });

                    if GITHUB_CLIENT_ID.is_some() || GITLAB_CLIENT_ID.is_some() {
                        ui.add_space(layout.spacing.sm);
                        ui.label(
                            RichText::new("Note: this build has a built-in Client ID (GITSPACE_GITHUB_CLIENT_ID / GITSPACE_GITLAB_CLIENT_ID). It is used instead of the one entered here.")
                                .color(layout.theme.palette.text_secondary)
                                .small(),
                        );
                    }
                });
            },
        );
    }

    fn manual_entry_section(&mut self, ui: &mut Ui, layout: &AuthLayout<'_>) {
        ui.collapsing("Advanced: Manual Token Entry", |ui| {
            ui.add_space(layout.spacing.sm);
            ui.label(
                RichText::new(
                    "For advanced users: manually enter a Personal Access Token.\n\
                     Useful for self-hosted instances or when OAuth is not available.",
                )
                .color(layout.theme.palette.text_secondary),
            );
            ui.add_space(layout.spacing.md);

            ui.horizontal(|ui| {
                ui.label(RichText::new("Host:").color(layout.theme.palette.text_secondary));
                ui.add(
                    TextEdit::singleline(&mut self.manual_host)
                        .hint_text("e.g., github.com or gitlab.example.com")
                        .desired_width(200.0),
                );
            });

            ui.add_space(layout.spacing.sm);

            ui.horizontal(|ui| {
                ui.label(RichText::new("Token:").color(layout.theme.palette.text_secondary));
                ui.add(
                    TextEdit::singleline(&mut self.manual_token)
                        .hint_text("Personal Access Token")
                        .password(true)
                        .desired_width(300.0),
                );
            });

            ui.add_space(layout.spacing.sm);

            let can_save = !self.manual_host.trim().is_empty()
                && !self.manual_token.trim().is_empty()
                && self.manual_validation.is_none();

            if ui
                .add_enabled(can_save, egui::Button::new("Validate & Save"))
                .clicked()
            {
                self.start_manual_validation();
            }

            if let Some(status) = &self.manual_status {
                ui.add_space(layout.spacing.sm);
                ui.label(RichText::new(status).color(layout.theme.palette.text_secondary));
            }
        });
    }

    /// Shows the GitHub user code with copy, open and cancel actions while a sign-in runs.
    fn github_device_code(&mut self, ui: &mut Ui, layout: &AuthLayout<'_>) {
        let Some(sign_in) = &self.github_sign_in else {
            return;
        };
        let mut cancel = false;
        let mut open_error = None;

        ui.add_space(layout.spacing.md);
        match sign_in {
            GithubSignIn::Requesting(_) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new("Asking GitHub for a sign-in code...")
                            .color(layout.theme.palette.text_secondary),
                    );
                    cancel = ui.button("Cancel").clicked();
                });
            }
            GithubSignIn::Waiting {
                user_code,
                verification_uri,
                ..
            } => {
                ui.label(
                    RichText::new(format!("Enter this code at {verification_uri}:"))
                        .color(layout.theme.palette.text_secondary),
                );
                ui.add_space(layout.spacing.xs);
                ui.label(
                    RichText::new(user_code.as_str())
                        .monospace()
                        .size(layout.theme.typography.heading)
                        .strong()
                        .color(layout.theme.palette.text_primary),
                );
                ui.add_space(layout.spacing.xs);
                ui.horizontal(|ui| {
                    if ui.button("Copy code").clicked() {
                        ui.ctx().copy_text(user_code.clone());
                    }
                    if ui.button("Open in browser").clicked() {
                        if let Err(err) = webbrowser::open(verification_uri) {
                            open_error = Some(format!("Failed to open browser: {err}"));
                        }
                    }
                    cancel = ui.button("Cancel").clicked();
                });
                ui.add_space(layout.spacing.xs);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new("Waiting for you to approve GitSpace on GitHub...")
                            .color(layout.theme.palette.text_secondary),
                    );
                });
            }
        }

        if cancel {
            // Dropping the sign-in drops its cancel sender, which stops the polling thread.
            self.github_sign_in = None;
            self.github_status = Some("Login cancelled".to_string());
        } else if open_error.is_some() {
            self.github_status = open_error;
        }
    }

    fn start_github_oauth(&mut self) {
        let client_id = if let Some(id) = GITHUB_CLIENT_ID {
            id.to_string()
        } else if !self.custom_github_client_id.trim().is_empty() {
            self.custom_github_client_id.trim().to_string()
        } else {
            self.github_status = Some("Please configure a GitHub Client ID first".to_string());
            return;
        };

        let provider = OAuthProvider::github(&client_id);
        self.github_status = None;
        self.github_sign_in = Some(GithubSignIn::Requesting(Promise::spawn_thread(
            "github_device_code",
            move || DeviceFlow::start(provider).map(Box::new),
        )));
    }

    fn start_gitlab_oauth(&mut self) {
        let client_id = if let Some(id) = GITLAB_CLIENT_ID {
            id.to_string()
        } else if !self.custom_gitlab_client_id.trim().is_empty() {
            self.custom_gitlab_client_id.trim().to_string()
        } else {
            self.gitlab_status = Some("Please configure a GitLab Client ID first".to_string());
            return;
        };

        let provider = OAuthProvider::gitlab(&client_id);
        let auth = self.auth.clone();

        self.gitlab_status = Some("Opening browser for login...".to_string());
        self.gitlab_oauth_promise = Some(Promise::spawn_thread("gitlab_oauth", move || {
            auth.start_oauth(provider)
        }));
    }

    fn poll_github_sign_in(&mut self) {
        match self.github_sign_in.take() {
            None => {}
            Some(GithubSignIn::Requesting(promise)) => match promise.try_take() {
                Ok(Ok(flow)) => {
                    let (cancel, cancelled) = mpsc::channel();
                    let user_code = flow.user_code().to_string();
                    let verification_uri = flow.verification_uri().to_string();
                    let result =
                        Promise::spawn_thread("github_device_flow", move || flow.wait(&cancelled));
                    self.github_sign_in = Some(GithubSignIn::Waiting {
                        user_code,
                        verification_uri,
                        result,
                        cancel,
                    });
                }
                Ok(Err(e)) => self.github_status = Some(format!("Login failed: {}", e)),
                Err(promise) => self.github_sign_in = Some(GithubSignIn::Requesting(promise)),
            },
            Some(GithubSignIn::Waiting {
                user_code,
                verification_uri,
                result,
                cancel,
            }) => match result.try_take() {
                Ok(result) => self.github_status = Some(self.finish_oauth(result)),
                Err(result) => {
                    self.github_sign_in = Some(GithubSignIn::Waiting {
                        user_code,
                        verification_uri,
                        result,
                        cancel,
                    })
                }
            },
        }
    }

    fn poll_gitlab_oauth(&mut self) {
        let gitlab_result = self
            .gitlab_oauth_promise
            .as_ref()
            .and_then(|p| p.ready().cloned());

        if let Some(result) = gitlab_result {
            self.gitlab_oauth_promise = None;
            self.gitlab_status = Some(self.finish_oauth(result));
        }
    }

    /// Stores the token of a finished OAuth sign-in and returns the status to show.
    fn finish_oauth(&self, result: OAuthResult) -> String {
        match result {
            OAuthResult::Success(token) => match self.auth.store_oauth_token(&token) {
                Ok(()) => "Successfully connected!".to_string(),
                Err(e) => format!("Failed to store token: {}", e),
            },
            OAuthResult::Cancelled => "Login cancelled".to_string(),
            OAuthResult::Error(e) => format!("Login failed: {}", e),
        }
    }

    fn start_manual_validation(&mut self) {
        let host = self.manual_host.trim().to_string();
        let token = self.manual_token.trim().to_string();
        let auth = self.auth.clone();

        self.manual_status = Some("Validating token...".to_string());
        self.manual_validation = Some(Promise::spawn_thread("validate_token", move || {
            auth.validate_and_store(&host, &token)
        }));
    }

    fn poll_manual_validation(&mut self) {
        if let Some(promise) = &self.manual_validation {
            if let Some(result) = promise.ready() {
                let result = result.clone();
                self.manual_validation = None;
                match result {
                    Ok(_) => {
                        self.manual_status = Some("Token validated and saved!".to_string());
                        self.manual_token.clear();
                    }
                    Err(e) => {
                        self.manual_status = Some(format!("Validation failed: {}", e));
                    }
                }
            }
        }
    }
}

struct AuthLayout<'a> {
    theme: &'a SharedTheme,
    spacing: crate::ui::theme::Spacing,
}

impl<'a> AuthLayout<'a> {
    fn new(theme: &'a SharedTheme) -> Self {
        Self {
            theme,
            spacing: theme.spacing,
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
            .stroke(egui::Stroke::new(
                1.0_f32,
                self.theme.palette.surface_highlight,
            ))
            .inner_margin(egui::Margin::same(self.spacing.md))
            .rounding(egui::Rounding::same(self.spacing.xs));
        frame.show(ui, |ui| {
            section.header(ui, self.theme, self.spacing);
            ui.add_space(self.spacing.md);
            content(ui);
        });
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
            subtitle: None,
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
        ui.add(egui::Button::new(text).fill(fill))
    }
}
