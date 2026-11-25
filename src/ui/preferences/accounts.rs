//! Preferences → Accounts: the saved accounts, GitHub's device flow, GitLab's browser
//! sign-in (OAuth2 PKCE) and personal access tokens.

use std::sync::mpsc::{self, Receiver, Sender};

use colony_ui::theme::Palette;
use colony_ui::Typography;
use iced::futures::{SinkExt, Stream};
use iced::widget::{column, container, row, text, text_input, Column};
use iced::{Alignment, Border, Element, Length, Task};

use crate::auth::{AuthManager, DeviceFlow, OAuthProvider, OAuthResult};
use crate::i18n::{t, tf};
use crate::ui::changes::field;
use crate::ui::history::{avatar, avatar_accent, initials};
use crate::ui::widgets::button as gs_button;
use crate::ui::widgets::chip::{chip, Tone};

/// OAuth client IDs set at build time. When set, they replace the ID typed in Preferences.
///
/// GitHub (device flow): an OAuth app at https://github.com/settings/developers with
/// "Enable Device Flow" ticked; the callback URL is not used.
/// GitLab (PKCE): an application at https://gitlab.com/-/user_settings/applications with
/// "Confidential" unticked and the redirect URI http://127.0.0.1:9876/callback.
const GITHUB_CLIENT_ID: Option<&str> = option_env!("GITSPACE_GITHUB_CLIENT_ID");
const GITLAB_CLIENT_ID: Option<&str> = option_env!("GITSPACE_GITLAB_CLIENT_ID");

/// A status line: a label key and the error or detail its `{error}` placeholder takes.
/// Translated when drawn, so a language change shows at once.
type Status = Option<(&'static str, String)>;

#[derive(Debug, Clone)]
pub enum Message {
    StartGithubOAuth,
    StartGitlabOAuth,
    /// A step of a GitHub sign-in, with the attempt's id.
    GithubSignIn(u64, GithubSignInEvent),
    CancelGithubSignIn,
    CopyGithubCode,
    OpenGithubVerification,
    GitlabOAuthResult(OAuthResult),
    ManualHostChanged(String),
    ManualTokenChanged(String),
    ValidateAndSave,
    ValidationResult(Result<(), String>),
    GithubClientIdChanged(String),
    GitlabClientIdChanged(String),
    /// Removes the account saved for a host.
    Disconnect(String),
    Disconnected(Result<(), String>),
    /// A received OAuth token was saved, or not.
    OAuthStored(OAuthService, Result<(), String>),
    /// The saved accounts, read again: host and whether it holds an OAuth token.
    AccountsLoaded(Vec<(String, bool)>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthService {
    GitHub,
    GitLab,
}

/// A step sent by the background task of a GitHub sign-in.
#[derive(Debug, Clone)]
pub enum GithubSignInEvent {
    /// GitHub gave the code to enter on its verification page.
    Code {
        user_code: String,
        verification_uri: String,
    },
    /// The sign-in ended: accepted, refused, expired or cancelled.
    Finished(OAuthResult),
}

/// A GitHub sign-in in progress (device flow).
#[derive(Debug, Clone)]
struct GithubSignIn {
    /// The attempt's id: steps of a cancelled attempt are dropped.
    id: u64,
    /// The code and verification page, once GitHub sent them.
    code: Option<(String, String)>,
    /// Dropping it stops the background wait for the token.
    _cancel: Sender<()>,
}

#[derive(Debug, Clone)]
pub struct Accounts {
    auth: AuthManager,
    github_status: Status,
    gitlab_status: Status,
    github_sign_in: Option<GithubSignIn>,
    next_github_sign_in: u64,
    gitlab_in_progress: bool,
    manual_host: String,
    manual_token: String,
    manual_status: Status,
    manual_validating: bool,
    custom_github_client_id: String,
    custom_gitlab_client_id: String,
    /// Saved accounts (host, OAuth), read again in the background after every change.
    accounts: Vec<(String, bool)>,
    /// The error of the last disconnect.
    accounts_status: Status,
}

impl Accounts {
    pub fn new(auth: AuthManager) -> Self {
        Self {
            auth,
            github_status: None,
            gitlab_status: None,
            github_sign_in: None,
            next_github_sign_in: 0,
            gitlab_in_progress: false,
            manual_host: String::new(),
            manual_token: String::new(),
            manual_status: None,
            manual_validating: false,
            custom_github_client_id: String::new(),
            custom_gitlab_client_id: String::new(),
            accounts: Vec::new(),
            accounts_status: None,
        }
    }

    /// Replaces the auth manager and reads the accounts again.
    pub fn set_auth_manager(&mut self, auth: AuthManager) -> Task<Message> {
        self.auth = auth;
        self.refresh_accounts()
    }

    /// Reads the saved accounts off the UI thread: the host list reads a file and each
    /// OAuth check asks the keyring.
    pub fn refresh_accounts(&self) -> Task<Message> {
        let auth = self.auth.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    auth.known_hosts()
                        .into_iter()
                        .filter(|host| !host.starts_with("oauth:"))
                        .map(|host| {
                            let oauth = auth.has_oauth(&host);
                            (host, oauth)
                        })
                        .collect()
                })
                .await
                .unwrap_or_default()
            },
            Message::AccountsLoaded,
        )
    }

    /// Whether an account is saved for `host`, as of the last account read.
    pub fn is_connected(&self, host: &str) -> bool {
        self.accounts.iter().any(|(saved, _)| saved == host)
    }

    fn status_mut(&mut self, service: OAuthService) -> &mut Status {
        match service {
            OAuthService::GitHub => &mut self.github_status,
            OAuthService::GitLab => &mut self.gitlab_status,
        }
    }

    /// Ends an OAuth sign-in: a received token is saved in the background.
    fn finish_oauth(&mut self, service: OAuthService, result: OAuthResult) -> Task<Message> {
        let status = match result {
            OAuthResult::Success(token) => {
                *self.status_mut(service) = Some(("prefs_signin_saving", String::new()));
                let auth = self.auth.clone();
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || auth.store_oauth_token(&token))
                            .await
                            .unwrap_or_else(|err| Err(err.to_string()))
                    },
                    move |result| Message::OAuthStored(service, result),
                );
            }
            OAuthResult::Cancelled => ("prefs_signin_cancelled", String::new()),
            OAuthResult::Error(e) => ("prefs_signin_failed", e),
        };
        *self.status_mut(service) = Some(status);
        Task::none()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::StartGithubOAuth => {
                let Some(client_id) = client_id(GITHUB_CLIENT_ID, &self.custom_github_client_id)
                else {
                    self.github_status = Some(("prefs_no_client_id", String::new()));
                    return Task::none();
                };

                let id = self.next_github_sign_in;
                self.next_github_sign_in += 1;
                let (cancel, cancelled) = mpsc::channel();
                self.github_status = None;
                self.github_sign_in = Some(GithubSignIn {
                    id,
                    code: None,
                    _cancel: cancel,
                });

                Task::run(
                    github_device_flow(OAuthProvider::github(&client_id), cancelled),
                    move |event| Message::GithubSignIn(id, event),
                )
            }

            Message::StartGitlabOAuth => {
                let Some(client_id) = client_id(GITLAB_CLIENT_ID, &self.custom_gitlab_client_id)
                else {
                    self.gitlab_status = Some(("prefs_no_client_id", String::new()));
                    return Task::none();
                };

                self.gitlab_status = Some(("prefs_gitlab_opening", String::new()));
                self.gitlab_in_progress = true;

                let auth = self.auth.clone();
                let provider = OAuthProvider::gitlab(&client_id);

                Task::perform(
                    tokio::task::spawn_blocking(move || auth.start_oauth(provider)),
                    |result| match result {
                        Ok(oauth_result) => Message::GitlabOAuthResult(oauth_result),
                        Err(err) => Message::GitlabOAuthResult(OAuthResult::Error(err.to_string())),
                    },
                )
            }

            Message::GithubSignIn(id, event) => {
                if self.github_sign_in.as_ref().map(|sign_in| sign_in.id) != Some(id) {
                    return Task::none();
                }
                let result = match event {
                    GithubSignInEvent::Code {
                        user_code,
                        verification_uri,
                    } => {
                        if let Some(sign_in) = &mut self.github_sign_in {
                            sign_in.code = Some((user_code, verification_uri));
                        }
                        return Task::none();
                    }
                    GithubSignInEvent::Finished(result) => result,
                };
                self.github_sign_in = None;
                self.finish_oauth(OAuthService::GitHub, result)
            }

            Message::CancelGithubSignIn => {
                // Dropping the attempt closes its cancel channel and stops the wait.
                self.github_sign_in = None;
                self.github_status = Some(("prefs_signin_cancelled", String::new()));
                Task::none()
            }

            Message::CopyGithubCode => match self.github_code() {
                Some((user_code, _)) => iced::clipboard::write(user_code.to_string()),
                None => Task::none(),
            },

            Message::OpenGithubVerification => {
                if let Some((_, verification_uri)) = self.github_code() {
                    if let Err(err) = webbrowser::open(verification_uri) {
                        self.github_status = Some(("prefs_browser_failed", err.to_string()));
                    }
                }
                Task::none()
            }

            Message::GitlabOAuthResult(result) => {
                self.gitlab_in_progress = false;
                self.finish_oauth(OAuthService::GitLab, result)
            }

            Message::OAuthStored(service, result) => {
                *self.status_mut(service) = Some(match result {
                    Ok(()) => ("prefs_signin_done", String::new()),
                    Err(e) => ("prefs_token_store_failed", e),
                });
                self.refresh_accounts()
            }

            Message::ManualHostChanged(value) => {
                self.manual_host = value;
                Task::none()
            }

            Message::ManualTokenChanged(value) => {
                self.manual_token = value;
                Task::none()
            }

            Message::ValidateAndSave => {
                let host = self.manual_host.trim().to_string();
                let token = self.manual_token.trim().to_string();

                if host.is_empty() || token.is_empty() {
                    self.manual_status = Some(("prefs_token_empty", String::new()));
                    return Task::none();
                }

                self.manual_status = Some(("prefs_token_checking", String::new()));
                self.manual_validating = true;

                let auth = self.auth.clone();
                Task::perform(
                    async move { auth.validate_and_store(&host, &token).await },
                    Message::ValidationResult,
                )
            }

            Message::ValidationResult(result) => {
                self.manual_validating = false;
                match result {
                    Ok(()) => {
                        self.manual_status = Some(("prefs_token_saved", String::new()));
                        self.manual_token.clear();
                        return self.refresh_accounts();
                    }
                    Err(e) => {
                        self.manual_status = Some(("prefs_token_rejected", e));
                    }
                }
                Task::none()
            }

            Message::GithubClientIdChanged(value) => {
                self.custom_github_client_id = value;
                Task::none()
            }

            Message::GitlabClientIdChanged(value) => {
                self.custom_gitlab_client_id = value;
                Task::none()
            }

            Message::Disconnect(host) => {
                self.accounts_status = None;
                let auth = self.auth.clone();
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            // An OAuth account removes its OAuth token, any other its plain one.
                            if auth.has_oauth(&host) {
                                auth.clear_oauth_token(&host)
                            } else {
                                auth.clear_token(&host)
                            }
                        })
                        .await
                        .unwrap_or_else(|err| Err(err.to_string()))
                    },
                    Message::Disconnected,
                )
            }

            Message::Disconnected(result) => {
                if let Err(e) = result {
                    self.accounts_status = Some(("prefs_disconnect_failed", e));
                }
                self.refresh_accounts()
            }

            Message::AccountsLoaded(accounts) => {
                self.accounts = accounts;
                Task::none()
            }
        }
    }

    /// The GitHub code waiting to be entered, and its verification page.
    fn github_code(&self) -> Option<(&str, &str)> {
        let (user_code, verification_uri) = self.github_sign_in.as_ref()?.code.as_ref()?;
        Some((user_code, verification_uri))
    }

    // ─── View ─────────────────────────────────────────────────────────

    /// One card per saved account, with its Disconnect button.
    pub fn cards(&self) -> Element<'_, Message> {
        let typo = Typography::current();
        let mut cards = Column::new().spacing(8);
        if self.accounts.is_empty() {
            cards = cards.push(muted(&typo, t("prefs_accounts_none")));
        }
        for (host, oauth) in &self.accounts {
            let how = if *oauth {
                t("prefs_account_oauth")
            } else {
                t("prefs_account_token")
            };
            let card = row![
                avatar(&provider_initials(host), avatar_accent(host), 30.0),
                column![
                    text(host.clone())
                        .size(typo.sz(13))
                        .font(typo.regular)
                        .color(Palette::TEXT_PRIMARY()),
                    text(how)
                        .size(typo.sz(11))
                        .font(typo.regular)
                        .color(Palette::TEXT_DIMMER()),
                ]
                .spacing(2)
                .width(Length::Fill),
                chip(t("prefs_connected"), Tone::Success),
                gs_button::danger(
                    t("prefs_disconnect"),
                    Some(Message::Disconnect(host.clone()))
                ),
            ]
            .spacing(12)
            .align_y(Alignment::Center);
            cards = cards.push(
                container(card)
                    .padding([12, 14])
                    .style(|_| container::Style {
                        background: Some(Palette::BG_SIDEBAR().into()),
                        border: Border::default().rounded(10),
                        ..Default::default()
                    }),
            );
        }
        cards = push_status(cards, &typo, &self.accounts_status);
        cards.into()
    }

    /// Sign in to GitHub: the start button, or the code with Copy, Open and Cancel.
    pub fn github(&self) -> Element<'_, Message> {
        let typo = Typography::current();
        let body = match &self.github_sign_in {
            None => {
                let available = client_id(GITHUB_CLIENT_ID, &self.custom_github_client_id);
                let start = column![gs_button::primary(
                    t("prefs_github_start"),
                    available.as_ref().map(|_| Message::StartGithubOAuth),
                )];
                match available {
                    Some(_) => start,
                    None => start.push(dimmer(&typo, t("prefs_no_client_id"))),
                }
            }
            Some(GithubSignIn { code: None, .. }) => column![row![
                muted(&typo, t("prefs_github_requesting")),
                gs_button::secondary(t("prefs_cancel"), Some(Message::CancelGithubSignIn)),
            ]
            .spacing(12)
            .align_y(Alignment::Center)],
            Some(GithubSignIn {
                code: Some((user_code, verification_uri)),
                ..
            }) => {
                let code = container(
                    text(user_code.clone())
                        .size(typo.sz(22))
                        .font(typo.bold)
                        .color(Palette::TEXT_PRIMARY()),
                )
                .padding([10, 16])
                .style(|_| container::Style {
                    background: Some(Palette::BG_SIDEBAR().into()),
                    border: Border::default().rounded(8),
                    ..Default::default()
                });
                column![
                    muted(
                        &typo,
                        &tf("prefs_github_enter_code", &[("uri", verification_uri)])
                    ),
                    row![
                        code,
                        gs_button::secondary(t("prefs_copy"), Some(Message::CopyGithubCode)),
                        gs_button::primary(
                            t("prefs_open_github"),
                            Some(Message::OpenGithubVerification)
                        ),
                        gs_button::secondary(t("prefs_cancel"), Some(Message::CancelGithubSignIn)),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                    dimmer(&typo, t("prefs_github_waiting")),
                ]
            }
        };
        push_status(body.spacing(10), &typo, &self.github_status).into()
    }

    /// Sign in to GitLab: the browser sign-in button and its status.
    pub fn gitlab(&self) -> Element<'_, Message> {
        let typo = Typography::current();
        let has_id = client_id(GITLAB_CLIENT_ID, &self.custom_gitlab_client_id).is_some();
        let mut body = column![gs_button::primary(
            t("prefs_gitlab_start"),
            (has_id && !self.gitlab_in_progress).then_some(Message::StartGitlabOAuth),
        )]
        .spacing(10);
        if !has_id {
            body = body.push(dimmer(&typo, t("prefs_no_client_id")));
        }
        push_status(body, &typo, &self.gitlab_status).into()
    }

    /// Add a token: host and token fields, then Validate and save.
    pub fn token(&self) -> Element<'_, Message> {
        let typo = Typography::current();
        let can_save = !self.manual_host.trim().is_empty()
            && !self.manual_token.trim().is_empty()
            && !self.manual_validating;
        let body = column![
            field(
                text_input(t("prefs_token_host"), &self.manual_host)
                    .on_input(Message::ManualHostChanged),
                None,
            ),
            field(
                text_input(t("prefs_token_value"), &self.manual_token)
                    .on_input(Message::ManualTokenChanged)
                    .secure(true),
                None,
            ),
            gs_button::primary(
                t("prefs_token_save"),
                can_save.then_some(Message::ValidateAndSave)
            ),
        ]
        .spacing(10);
        push_status(body, &typo, &self.manual_status).into()
    }

    /// OAuth app client IDs: one field per provider.
    pub fn client_ids(&self) -> Element<'_, Message> {
        let typo = Typography::current();
        let mut body = column![
            field(
                text_input(t("prefs_github_client_id"), &self.custom_github_client_id)
                    .on_input(Message::GithubClientIdChanged),
                None,
            ),
            field(
                text_input(t("prefs_gitlab_client_id"), &self.custom_gitlab_client_id)
                    .on_input(Message::GitlabClientIdChanged),
                None,
            ),
        ]
        .spacing(10);
        if GITHUB_CLIENT_ID.is_some() || GITLAB_CLIENT_ID.is_some() {
            body = body.push(dimmer(&typo, t("prefs_client_ids_built_in")));
        }
        body.into()
    }
}

/// The client ID to sign in with: the built-in one, else the one typed in Preferences.
fn client_id(built_in: Option<&str>, typed: &str) -> Option<String> {
    built_in
        .map(str::to_string)
        .or_else(|| Some(typed.trim().to_string()).filter(|id| !id.is_empty()))
}

/// "GH" for GitHub, "GL" for GitLab, else the host's own initials.
fn provider_initials(host: &str) -> String {
    if host.contains("github") {
        "GH".to_string()
    } else if host.contains("gitlab") {
        "GL".to_string()
    } else {
        initials(host)
    }
}

fn muted<'a>(typo: &Typography, label: &str) -> Element<'a, Message> {
    text(label.to_string())
        .size(typo.sz(12))
        .font(typo.regular)
        .color(Palette::TEXT_MUTED())
        .into()
}

fn dimmer<'a>(typo: &Typography, label: &str) -> Element<'a, Message> {
    text(label.to_string())
        .size(typo.sz(11))
        .font(typo.regular)
        .color(Palette::TEXT_DIMMEST())
        .into()
}

fn push_status<'a>(
    body: Column<'a, Message>,
    typo: &Typography,
    status: &Status,
) -> Column<'a, Message> {
    match status {
        Some((key, detail)) => body.push(muted(typo, &tf(key, &[("error", detail)]))),
        None => body,
    }
}

/// GitHub sign-in by device flow: asks for a code, sends it to the UI, then waits until the
/// user accepts or refuses it, the code expires or `cancelled` is closed.
fn github_device_flow(
    provider: OAuthProvider,
    cancelled: Receiver<()>,
) -> impl Stream<Item = GithubSignInEvent> {
    iced::stream::channel(2, async move |mut output| {
        let flow = match tokio::task::spawn_blocking(move || DeviceFlow::start(provider)).await {
            Ok(Ok(flow)) => flow,
            Ok(Err(err)) => {
                let _ = output
                    .send(GithubSignInEvent::Finished(OAuthResult::Error(err)))
                    .await;
                return;
            }
            Err(err) => {
                let _ = output
                    .send(GithubSignInEvent::Finished(OAuthResult::Error(
                        err.to_string(),
                    )))
                    .await;
                return;
            }
        };
        let _ = output
            .send(GithubSignInEvent::Code {
                user_code: flow.user_code().to_string(),
                verification_uri: flow.verification_uri().to_string(),
            })
            .await;
        let result = tokio::task::spawn_blocking(move || flow.wait(&cancelled))
            .await
            .unwrap_or_else(|err| OAuthResult::Error(err.to_string()));
        let _ = output.send(GithubSignInEvent::Finished(result)).await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_client_id_wins_and_blank_ids_do_not_count() {
        assert_eq!(client_id(Some("built"), "typed").as_deref(), Some("built"));
        assert_eq!(client_id(None, "  typed ").as_deref(), Some("typed"));
        assert_eq!(client_id(None, "   "), None);
        assert_eq!(provider_initials("github.com"), "GH");
        assert_eq!(provider_initials("gitlab.example.com"), "GL");
        assert_eq!(provider_initials("gitea.example.com"), "GE");
    }
}
