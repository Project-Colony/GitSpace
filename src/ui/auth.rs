//! Panneau d'authentification avec flux OAuth et saisie manuelle de tokens.
//!
//! Gère la connexion à GitHub (device flow), à GitLab (OAuth2 PKCE)
//! ou via la saisie manuelle d'un jeton d'accès personnel.

use std::sync::mpsc::{self, Receiver, Sender};

use iced::futures::{SinkExt, Stream};
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length, Task};

use crate::auth::{AuthManager, DeviceFlow, OAuthProvider, OAuthResult};
use crate::ui::theme::Theme;

/// Identifiants client OAuth configurés au moment de la compilation. Quand ils sont définis,
/// ils remplacent l'identifiant saisi dans l'interface.
///
/// GitHub (device flow) : une application OAuth sur https://github.com/settings/developers avec
/// « Enable Device Flow » coché ; l'URL de callback n'est pas utilisée.
/// GitLab (PKCE) : une application sur https://gitlab.com/-/user_settings/applications avec
/// « Confidential » décoché et l'URI de redirection http://127.0.0.1:9876/callback.
const GITHUB_CLIENT_ID: Option<&str> = option_env!("GITSPACE_GITHUB_CLIENT_ID");
const GITLAB_CLIENT_ID: Option<&str> = option_env!("GITSPACE_GITLAB_CLIENT_ID");

/// Messages émis par le panneau d'authentification.
#[derive(Debug, Clone)]
pub enum Message {
    /// Démarrer le flux OAuth GitHub.
    StartGithubOAuth,
    /// Démarrer le flux OAuth GitLab.
    StartGitlabOAuth,
    /// Étape d'une connexion GitHub, avec l'identifiant de la tentative.
    GithubSignIn(u64, GithubSignInEvent),
    /// Annuler la connexion GitHub en cours.
    CancelGithubSignIn,
    /// Copier le code GitHub dans le presse-papiers.
    CopyGithubCode,
    /// Ouvrir la page de vérification GitHub dans le navigateur.
    OpenGithubVerification,
    /// Résultat du flux OAuth GitLab.
    GitlabOAuthResult(OAuthResult),
    /// Le champ hôte manuel a changé.
    ManualHostChanged(String),
    /// Le champ token manuel a changé.
    ManualTokenChanged(String),
    /// Valider et enregistrer le token saisi manuellement.
    ValidateAndSave,
    /// Résultat de la validation du token manuel.
    ValidationResult(Result<(), String>),
    /// Identifiant client GitHub personnalisé modifié.
    GithubClientIdChanged(String),
    /// Identifiant client GitLab personnalisé modifié.
    GitlabClientIdChanged(String),
    /// Déconnecter un compte (par hôte).
    Disconnect(String),
    /// Résultat de la déconnexion d'un compte.
    Disconnected(Result<(), String>),
    /// Résultat de l'enregistrement d'un token OAuth reçu.
    OAuthStored(OAuthService, Result<(), String>),
    /// Comptes enregistrés relus : hôte et présence d'un token OAuth.
    AccountsLoaded(Vec<(String, bool)>),
}

/// Fournisseur d'une connexion OAuth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthService {
    GitHub,
    GitLab,
}

impl OAuthService {
    fn label(self) -> &'static str {
        match self {
            Self::GitHub => "GitHub",
            Self::GitLab => "GitLab",
        }
    }
}

/// Étape envoyée par la tâche de fond d'une connexion GitHub.
#[derive(Debug, Clone)]
pub enum GithubSignInEvent {
    /// GitHub a fourni le code à saisir sur la page de vérification.
    Code {
        user_code: String,
        verification_uri: String,
    },
    /// La connexion est terminée, acceptée, refusée, expirée ou annulée.
    Finished(OAuthResult),
}

/// Connexion GitHub en cours (device flow).
#[derive(Debug, Clone)]
struct GithubSignIn {
    /// Identifiant de la tentative : les étapes d'une tentative annulée sont ignorées.
    id: u64,
    /// Code et page de vérification, une fois reçus de GitHub.
    code: Option<(String, String)>,
    /// Le supprimer arrête l'attente du jeton côté tâche de fond.
    _cancel: Sender<()>,
}

/// État du panneau d'authentification.
#[derive(Debug, Clone)]
pub struct AuthPanel {
    /// Gestionnaire d'authentification partagé.
    auth: AuthManager,
    /// Message de statut pour GitHub.
    github_status: Option<String>,
    /// Message de statut pour GitLab.
    gitlab_status: Option<String>,
    /// Connexion GitHub en cours.
    github_sign_in: Option<GithubSignIn>,
    /// Identifiant de la prochaine tentative de connexion GitHub.
    next_github_sign_in: u64,
    /// Indique si un flux OAuth GitLab est en cours.
    gitlab_in_progress: bool,
    /// Hôte saisi manuellement.
    manual_host: String,
    /// Token saisi manuellement.
    manual_token: String,
    /// Message de statut pour la saisie manuelle.
    manual_status: Option<String>,
    /// Indique si une validation manuelle est en cours.
    manual_validating: bool,
    /// Identifiant client GitHub personnalisé.
    custom_github_client_id: String,
    /// Identifiant client GitLab personnalisé.
    custom_gitlab_client_id: String,
    /// Comptes enregistrés (hôte, OAuth), relus en tâche de fond après chaque changement.
    accounts: Vec<(String, bool)>,
    /// Erreur de la dernière déconnexion.
    accounts_status: Option<String>,
}

impl AuthPanel {
    /// Crée un nouveau panneau d'authentification.
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

    /// Met à jour le gestionnaire d'authentification et relit les comptes.
    pub fn set_auth_manager(&mut self, auth: AuthManager) -> Task<Message> {
        self.auth = auth;
        self.refresh_accounts()
    }

    /// Relit les comptes enregistrés hors du thread UI : la liste des hôtes lit un fichier et
    /// chaque test OAuth interroge le trousseau.
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

    /// Message de statut de la connexion OAuth d'un fournisseur.
    fn status_mut(&mut self, service: OAuthService) -> &mut Option<String> {
        match service {
            OAuthService::GitHub => &mut self.github_status,
            OAuthService::GitLab => &mut self.gitlab_status,
        }
    }

    /// Traite la fin d'une connexion OAuth : un token reçu est enregistré en tâche de fond.
    fn finish_oauth(&mut self, service: OAuthService, result: OAuthResult) -> Task<Message> {
        let status = match result {
            OAuthResult::Success(token) => {
                *self.status_mut(service) = Some("Enregistrement du token...".to_string());
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
            OAuthResult::Cancelled => "Connexion annulée".to_string(),
            OAuthResult::Error(e) => format!("Erreur de connexion : {e}"),
        };
        *self.status_mut(service) = Some(status);
        Task::none()
    }

    /// Traite un message et renvoie une tâche Iced.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::StartGithubOAuth => {
                let client_id = if let Some(id) = GITHUB_CLIENT_ID {
                    id.to_string()
                } else if !self.custom_github_client_id.trim().is_empty() {
                    self.custom_github_client_id.trim().to_string()
                } else {
                    self.github_status =
                        Some("Veuillez configurer un identifiant client GitHub".to_string());
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
                let client_id = if let Some(id) = GITLAB_CLIENT_ID {
                    id.to_string()
                } else if !self.custom_gitlab_client_id.trim().is_empty() {
                    self.custom_gitlab_client_id.trim().to_string()
                } else {
                    self.gitlab_status =
                        Some("Veuillez configurer un identifiant client GitLab".to_string());
                    return Task::none();
                };

                self.gitlab_status =
                    Some("Ouverture du navigateur pour la connexion...".to_string());
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
                // Supprimer la tentative ferme son canal d'annulation et arrête l'attente.
                self.github_sign_in = None;
                self.github_status = Some("Connexion annulée".to_string());
                Task::none()
            }

            Message::CopyGithubCode => match self.github_code() {
                Some((user_code, _)) => iced::clipboard::write(user_code.to_string()),
                None => Task::none(),
            },

            Message::OpenGithubVerification => {
                if let Some((_, verification_uri)) = self.github_code() {
                    if let Err(err) = webbrowser::open(verification_uri) {
                        self.github_status =
                            Some(format!("Impossible d'ouvrir le navigateur : {err}"));
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
                    Ok(()) => format!("Connexion {} réussie !", service.label()),
                    Err(e) => format!("Erreur de stockage du token : {e}"),
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
                    self.manual_status =
                        Some("L'hôte et le token ne peuvent pas être vides".to_string());
                    return Task::none();
                }

                self.manual_status = Some("Validation du token en cours...".to_string());
                self.manual_validating = true;

                let auth = self.auth.clone();

                // Valider et stocker le token de manière asynchrone.
                Task::perform(
                    async move { auth.validate_and_store(&host, &token).await },
                    Message::ValidationResult,
                )
            }

            Message::ValidationResult(result) => {
                self.manual_validating = false;
                match result {
                    Ok(()) => {
                        self.manual_status = Some("Token validé et enregistré !".to_string());
                        self.manual_token.clear();
                        return self.refresh_accounts();
                    }
                    Err(e) => {
                        self.manual_status = Some(format!("Validation échouée : {e}"));
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
                            // Supprimer le token OAuth ou le token simple selon le cas.
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
                    self.accounts_status = Some(format!("Déconnexion échouée : {e}"));
                }
                self.refresh_accounts()
            }

            Message::AccountsLoaded(accounts) => {
                self.accounts = accounts;
                Task::none()
            }
        }
    }

    /// Construit la vue Iced du panneau d'authentification.
    pub fn view<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        // En-tête du panneau.
        let heading = text("Authentification")
            .size(typo.heading)
            .color(palette.text_primary);

        let description =
            text("Connectez vos comptes de fournisseurs Git pour un acces fluide aux depots.")
                .size(typo.body)
                .color(palette.text_secondary);

        // Section des comptes connectés.
        let accounts_section = self.view_connected_accounts(theme);

        // Section des boutons OAuth.
        let oauth_section = self.view_oauth_section(theme);

        // Section de saisie manuelle.
        let manual_section = self.view_manual_section(theme);

        let content = column![
            heading,
            description,
            Space::new().height(theme.spacing.lg),
            accounts_section,
            Space::new().height(theme.spacing.lg),
            oauth_section,
            Space::new().height(theme.spacing.lg),
            manual_section,
        ]
        .spacing(theme.spacing.xs)
        .width(Length::Fill);

        scrollable(
            container(content)
                .width(Length::Fill)
                .padding(theme.spacing.md),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// Construit la section des comptes connectés.
    fn view_connected_accounts<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let title = text("Comptes connectes")
            .size(typo.title)
            .color(palette.text_primary);

        let subtitle = text("Vos fournisseurs Git authentifies")
            .size(typo.label)
            .color(palette.text_secondary);

        let mut items = column![].spacing(theme.spacing.xs);

        if self.accounts.is_empty() {
            items = items.push(
                text("Aucun compte connecte. Utilisez les boutons ci-dessous pour vous connecter.")
                    .size(typo.body)
                    .color(palette.text_secondary),
            );
        } else {
            for (host, has_oauth) in &self.accounts {
                let auth_type = if *has_oauth { "OAuth" } else { "Token" };

                // Icône selon le fournisseur.
                let icon_str = if host.contains("github") {
                    "GitHub"
                } else if host.contains("gitlab") {
                    "GitLab"
                } else {
                    "Git"
                };

                let label = text(format!("{icon_str} - {host} ({auth_type})"))
                    .size(typo.body)
                    .color(palette.text_primary);

                let disconnect_btn = button(
                    text("Deconnecter")
                        .size(typo.label)
                        .color(palette.text_primary),
                )
                .on_press(Message::Disconnect(host.to_string()));

                let account_row = row![label, Space::new().width(Length::Fill), disconnect_btn]
                    .spacing(theme.spacing.sm)
                    .align_y(Alignment::Center);

                items = items.push(account_row);
            }
        }
        if let Some(status) = &self.accounts_status {
            items = items.push(
                text(status.as_str())
                    .size(typo.label)
                    .color(palette.text_secondary),
            );
        }

        column![
            title,
            subtitle,
            Space::new().height(theme.spacing.sm),
            items
        ]
        .spacing(theme.spacing.xs)
        .into()
    }

    /// Code GitHub en attente de saisie et sa page de vérification.
    fn github_code(&self) -> Option<(&str, &str)> {
        let (user_code, verification_uri) = self.github_sign_in.as_ref()?.code.as_ref()?;
        Some((user_code, verification_uri))
    }

    /// Construit le bloc du code GitHub pendant une connexion.
    fn view_github_sign_in<'a>(&'a self, theme: &'a Theme) -> Option<Element<'a, Message>> {
        let sign_in = self.github_sign_in.as_ref()?;
        let palette = &theme.palette;
        let typo = &theme.typography;
        let small_button = |label: &'a str, message: Message| {
            button(text(label).size(typo.label).color(palette.text_primary)).on_press(message)
        };

        let content: Element<'a, Message> = match &sign_in.code {
            None => row![
                text("Demande d'un code de connexion à GitHub...")
                    .size(typo.body)
                    .color(palette.text_secondary),
                small_button("Annuler", Message::CancelGithubSignIn),
            ]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center)
            .into(),
            Some((user_code, verification_uri)) => column![
                text(format!("Saisissez ce code sur {verification_uri} :"))
                    .size(typo.body)
                    .color(palette.text_secondary),
                text(user_code.as_str())
                    .size(typo.heading)
                    .color(palette.text_primary),
                row![
                    small_button("Copier le code", Message::CopyGithubCode),
                    small_button("Ouvrir dans le navigateur", Message::OpenGithubVerification),
                    small_button("Annuler", Message::CancelGithubSignIn),
                ]
                .spacing(theme.spacing.sm),
                text("En attente de votre validation sur GitHub...")
                    .size(typo.label)
                    .color(palette.text_secondary),
            ]
            .spacing(theme.spacing.xs)
            .into(),
        };
        Some(content)
    }

    /// Construit la section des boutons de connexion OAuth.
    fn view_oauth_section<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let title = text("Connexion OAuth")
            .size(typo.title)
            .color(palette.text_primary);

        let description = text(
            "GitHub affiche un code à saisir sur sa page de connexion. GitLab ouvre une fenêtre de navigateur.",
        )
        .size(typo.body)
        .color(palette.text_secondary);

        // Bouton GitHub OAuth.
        let github_available = self.github_sign_in.is_none()
            && (GITHUB_CLIENT_ID.is_some() || !self.custom_github_client_id.trim().is_empty());

        let mut github_btn = button(
            text("Connexion GitHub")
                .size(typo.body)
                .color(palette.text_primary),
        );
        if github_available {
            github_btn = github_btn.on_press(Message::StartGithubOAuth);
        }

        // Bouton GitLab OAuth.
        let gitlab_available = !self.gitlab_in_progress
            && (GITLAB_CLIENT_ID.is_some() || !self.custom_gitlab_client_id.trim().is_empty());

        let mut gitlab_btn = button(
            text("Connexion GitLab")
                .size(typo.body)
                .color(palette.text_primary),
        );
        if gitlab_available {
            gitlab_btn = gitlab_btn.on_press(Message::StartGitlabOAuth);
        }

        let oauth_buttons = row![github_btn, Space::new().width(theme.spacing.md), gitlab_btn]
            .align_y(Alignment::Center);

        // Code GitHub puis messages de statut.
        let mut status_col = column![].spacing(theme.spacing.xs);
        if let Some(sign_in) = self.view_github_sign_in(theme) {
            status_col = status_col.push(sign_in);
        }
        if let Some(status) = &self.github_status {
            status_col = status_col.push(
                text(format!("GitHub : {status}"))
                    .size(typo.label)
                    .color(palette.text_secondary),
            );
        }
        if let Some(status) = &self.gitlab_status {
            status_col = status_col.push(
                text(format!("GitLab : {status}"))
                    .size(typo.label)
                    .color(palette.text_secondary),
            );
        }

        // Section de configuration des identifiants OAuth.
        let oauth_config = self.view_oauth_config(theme);

        column![
            title,
            description,
            Space::new().height(theme.spacing.sm),
            oauth_buttons,
            status_col,
            Space::new().height(theme.spacing.sm),
            oauth_config,
        ]
        .spacing(theme.spacing.xs)
        .into()
    }

    /// Construit la sous-section de configuration des identifiants OAuth.
    fn view_oauth_config<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let config_hint = text(
            "Pour utiliser OAuth, enregistrez une application OAuth chez votre fournisseur et entrez l'identifiant client.\n\
             GitHub : cochez « Enable Device Flow » dans l'application ; l'URL de callback n'est pas utilisée.\n\
             GitLab : décochez « Confidential » et indiquez l'URI de redirection http://127.0.0.1:9876/callback",
        )
        .size(typo.label)
        .color(palette.text_secondary);

        let github_label = text("GitHub Client ID :")
            .size(typo.label)
            .color(palette.text_secondary);

        let github_input = text_input("Identifiant OAuth GitHub", &self.custom_github_client_id)
            .on_input(Message::GithubClientIdChanged)
            .width(Length::Fixed(280.0));

        let github_row = row![github_label, github_input]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        let gitlab_label = text("GitLab Client ID :")
            .size(typo.label)
            .color(palette.text_secondary);

        let gitlab_input = text_input("Identifiant OAuth GitLab", &self.custom_gitlab_client_id)
            .on_input(Message::GitlabClientIdChanged)
            .width(Length::Fixed(280.0));

        let gitlab_row = row![gitlab_label, gitlab_input]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        // Note sur les variables d'environnement.
        let mut col = column![
            config_hint,
            Space::new().height(theme.spacing.xs),
            github_row,
            gitlab_row
        ]
        .spacing(theme.spacing.xs);

        if GITHUB_CLIENT_ID.is_some() || GITLAB_CLIENT_ID.is_some() {
            col = col.push(
                text(
                    "Note : des identifiants OAuth integres sont configures via \
                     GITSPACE_GITHUB_CLIENT_ID / GITSPACE_GITLAB_CLIENT_ID.",
                )
                .size(typo.label)
                .color(palette.text_secondary),
            );
        }

        col.into()
    }

    /// Construit la section de saisie manuelle de tokens.
    fn view_manual_section<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let title = text("Saisie manuelle de token")
            .size(typo.title)
            .color(palette.text_primary);

        let description = text(
            "Pour les utilisateurs avances : saisissez un jeton d'acces personnel.\n\
             Utile pour les instances auto-hebergees ou lorsque OAuth n'est pas disponible.",
        )
        .size(typo.body)
        .color(palette.text_secondary);

        // Champ hôte.
        let host_label = text("Hote :").size(typo.body).color(palette.text_secondary);

        let host_input = text_input("ex. github.com ou gitlab.example.com", &self.manual_host)
            .on_input(Message::ManualHostChanged)
            .width(Length::Fixed(280.0));

        let host_row = row![host_label, host_input]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        // Champ token (masqué).
        let token_label = text("Token :")
            .size(typo.body)
            .color(palette.text_secondary);

        let token_input = text_input("Jeton d'acces personnel", &self.manual_token)
            .on_input(Message::ManualTokenChanged)
            .secure(true)
            .width(Length::Fixed(300.0));

        let token_row = row![token_label, token_input]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        // Bouton de validation.
        let can_save = !self.manual_host.trim().is_empty()
            && !self.manual_token.trim().is_empty()
            && !self.manual_validating;

        let mut save_btn = button(
            text("Valider et enregistrer")
                .size(typo.body)
                .color(palette.text_primary),
        );
        if can_save {
            save_btn = save_btn.on_press(Message::ValidateAndSave);
        }

        // Message de statut.
        let mut col = column![
            title,
            description,
            Space::new().height(theme.spacing.sm),
            host_row,
            token_row,
            Space::new().height(theme.spacing.xs),
            save_btn,
        ]
        .spacing(theme.spacing.xs);

        if let Some(status) = &self.manual_status {
            col = col.push(
                text(status.as_str())
                    .size(typo.label)
                    .color(palette.text_secondary),
            );
        }

        col.into()
    }
}

/// Connexion GitHub par device flow : demande un code, l'envoie à l'interface, puis attend que
/// l'utilisateur l'accepte ou le refuse, que le code expire ou que `cancelled` soit fermé.
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
