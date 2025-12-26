use eframe::egui::{self, Align, Layout, RichText, TextEdit, Ui};
use poll_promise::Promise;

use crate::auth::AuthManager;
use crate::ui::theme::Theme;

pub struct AuthPanel {
    theme: Theme,
    auth: AuthManager,
    provider_gap: f32,
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
            provider_gap: DEFAULT_PROVIDER_GAP,
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
        ui.add_space(8.0);

        provider_section(
            ui,
            &self.theme,
            &self.auth,
            "GitHub",
            '\u{f408}',
            &mut self.github_host,
            &mut self.github_token,
            &mut self.github_status,
            &mut self.github_validation,
            "github.com",
        );
        self.provider_separator(ui);
        provider_section(
            ui,
            &self.theme,
            &self.auth,
            "GitLab",
            '\u{f296}',
            &mut self.gitlab_host,
            &mut self.gitlab_token,
            &mut self.gitlab_status,
            &mut self.gitlab_validation,
            "gitlab.com",
        );

        ui.add_space(12.0);
        ui.label(
            RichText::new("Examples (host only, no repository path):")
                .color(self.theme.palette.text_secondary),
        );
        ui.label(
            RichText::new("Remote host\ngithub.com\nExample: github.com/MotherSphere")
                .color(self.theme.palette.text_secondary),
        );
        ui.label(
            RichText::new("Remote host\ngitlab.com\nExample: gitlab.com/MotherSphere")
                .color(self.theme.palette.text_secondary),
        );

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
                        let message = Some(format!("Removed token for {}", host));
                        self.github_status = message.clone();
                        self.gitlab_status = message;
                    }
                });
            }
        }
    }

}

const MIN_PROVIDER_GAP: f32 = 4.0;
const DEFAULT_PROVIDER_GAP: f32 = 8.0;

impl AuthPanel {
    fn provider_separator(&mut self, ui: &mut Ui) {
        let gap_height = self.provider_gap.max(MIN_PROVIDER_GAP);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), gap_height),
            egui::Sense::hover(),
        );
        let grip_id = ui.make_persistent_id("auth_provider_separator_grip");
        let grip_response = ui.interact(rect, grip_id, egui::Sense::click_and_drag());
        if grip_response.hovered() || grip_response.dragged() {
            ui.output_mut(|output| output.cursor_icon = egui::CursorIcon::ResizeVertical);
        }

        if grip_response.dragged() {
            let delta = ui.input(|input| input.pointer.delta().y);
            self.provider_gap = (self.provider_gap + delta).max(MIN_PROVIDER_GAP);
        }

        let painter = ui.painter();
        let grip_line = egui::Stroke::new(1.0, self.theme.palette.surface_highlight);
        let center = rect.center();
        painter.line_segment(
            [egui::pos2(rect.left() + 12.0, center.y), egui::pos2(rect.right() - 12.0, center.y)],
            grip_line,
        );
    }
}

fn provider_section(
    ui: &mut Ui,
    theme: &Theme,
    auth: &AuthManager,
    label: &str,
    icon: char,
    host: &mut String,
    token: &mut String,
    status: &mut Option<String>,
    validation: &mut Option<Promise<Result<(), String>>>,
    host_hint: &str,
) {
    ui.label(
        RichText::new(format!("{icon} {label}"))
            .color(theme.palette.text_primary)
            .strong(),
    );
    ui.add_space(6.0);
    ui.label(
        RichText::new("Remote host")
            .color(theme.palette.text_secondary),
    );
    let control_height = ui.spacing().interact_size.y;
    ui.add_sized(
        [320.0, control_height],
        TextEdit::singleline(host).hint_text(host_hint),
    );

    ui.add_space(8.0);
    ui.label(RichText::new("Access Token").color(theme.palette.text_secondary));
    ui.add_sized(
        [360.0, control_height],
        TextEdit::singleline(token)
            .password(true)
            .hint_text("Paste your personal access token"),
    );

    let previous_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing = egui::vec2(previous_spacing.x, 0.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let button = ui.add_enabled(
            !host.trim().is_empty() && !token.trim().is_empty(),
            egui::Button::new("Validate & Save"),
        );
        if button.clicked() {
            start_validation(auth, host, token, status, validation);
        }
    });
    ui.spacing_mut().item_spacing = previous_spacing;

    if let Some(current_status) = status {
        ui.add_space(6.0);
        ui.colored_label(theme.palette.text_secondary, current_status);
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
