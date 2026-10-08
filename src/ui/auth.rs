//! Panneau d'authentification avec flux OAuth et saisie manuelle de tokens.
//!
//! Gère la connexion aux fournisseurs Git (GitHub, GitLab) via OAuth2 PKCE
//! ou via la saisie manuelle d'un jeton d'accès personnel.

use iced::widget::{
    button, column, container, row, scrollable, text, text_input, Space,
};
use iced::{Alignment, Element, Length, Task};

use crate::auth::{AuthManager, OAuthProvider, OAuthResult};
use crate::ui::theme::Theme;

/// Identifiants client OAuth configurés au moment de la compilation.
///
/// GitHub : https://github.com/settings/developers
/// GitLab : https://gitlab.com/-/profile/applications
///
/// URL de callback : http://127.0.0.1:9876/callback
const GITHUB_CLIENT_ID: Option<&str> = option_env!("GITSPACE_GITHUB_CLIENT_ID");
const GITLAB_CLIENT_ID: Option<&str> = option_env!("GITSPACE_GITLAB_CLIENT_ID");

/// Messages émis par le panneau d'authentification.
#[derive(Debug, Clone)]
pub enum Message {
    /// Démarrer le flux OAuth GitHub.
    StartGithubOAuth,
    /// Démarrer le flux OAuth GitLab.
    StartGitlabOAuth,
    /// Résultat du flux OAuth GitHub.
    GithubOAuthResult(OAuthResult),
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
    /// Indique si un flux OAuth GitHub est en cours.
    github_in_progress: bool,
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
}

impl AuthPanel {
    /// Crée un nouveau panneau d'authentification.
    pub fn new(auth: AuthManager) -> Self {
        Self {
            auth,
            github_status: None,
            gitlab_status: None,
            github_in_progress: false,
            gitlab_in_progress: false,
            manual_host: String::new(),
            manual_token: String::new(),
            manual_status: None,
            manual_validating: false,
            custom_github_client_id: String::new(),
            custom_gitlab_client_id: String::new(),
        }
    }

    /// Met à jour le gestionnaire d'authentification.
    pub fn set_auth_manager(&mut self, auth: AuthManager) {
        self.auth = auth;
    }

    /// Traite un message et renvoie une tâche Iced.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::StartGithubOAuth => {
                // Déterminer l'identifiant client à utiliser.
                let client_id = if !self.custom_github_client_id.is_empty() {
                    self.custom_github_client_id.clone()
                } else if let Some(id) = GITHUB_CLIENT_ID {
                    id.to_string()
                } else {
                    self.github_status =
                        Some("Veuillez configurer un identifiant client GitHub".to_string());
                    return Task::none();
                };

                self.github_status =
                    Some("Ouverture du navigateur pour la connexion...".to_string());
                self.github_in_progress = true;

                let auth = self.auth.clone();
                let provider = OAuthProvider::github(&client_id);

                // Lancer le flux OAuth dans un thread bloquant.
                Task::perform(
                    tokio::task::spawn_blocking(move || auth.start_oauth(provider)),
                    |result| match result {
                        Ok(oauth_result) => Message::GithubOAuthResult(oauth_result),
                        Err(err) => {
                            Message::GithubOAuthResult(OAuthResult::Error(err.to_string()))
                        }
                    },
                )
            }

            Message::StartGitlabOAuth => {
                let client_id = if !self.custom_gitlab_client_id.is_empty() {
                    self.custom_gitlab_client_id.clone()
                } else if let Some(id) = GITLAB_CLIENT_ID {
                    id.to_string()
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
                        Err(err) => {
                            Message::GitlabOAuthResult(OAuthResult::Error(err.to_string()))
                        }
                    },
                )
            }

            Message::GithubOAuthResult(result) => {
                self.github_in_progress = false;
                match result {
                    OAuthResult::Success(token) => {
                        match self.auth.store_oauth_token(&token) {
                            Ok(()) => {
                                self.github_status =
                                    Some("Connexion GitHub réussie !".to_string());
                            }
                            Err(e) => {
                                self.github_status =
                                    Some(format!("Erreur de stockage du token : {e}"));
                            }
                        }
                    }
                    OAuthResult::Cancelled => {
                        self.github_status = Some("Connexion annulée".to_string());
                    }
                    OAuthResult::Error(e) => {
                        self.github_status = Some(format!("Erreur de connexion : {e}"));
                    }
                }
                Task::none()
            }

            Message::GitlabOAuthResult(result) => {
                self.gitlab_in_progress = false;
                match result {
                    OAuthResult::Success(token) => {
                        match self.auth.store_oauth_token(&token) {
                            Ok(()) => {
                                self.gitlab_status =
                                    Some("Connexion GitLab réussie !".to_string());
                            }
                            Err(e) => {
                                self.gitlab_status =
                                    Some(format!("Erreur de stockage du token : {e}"));
                            }
                        }
                    }
                    OAuthResult::Cancelled => {
                        self.gitlab_status = Some("Connexion annulée".to_string());
                    }
                    OAuthResult::Error(e) => {
                        self.gitlab_status = Some(format!("Erreur de connexion : {e}"));
                    }
                }
                Task::none()
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
                        self.manual_status =
                            Some("Token validé et enregistré !".to_string());
                        self.manual_token.clear();
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
                // Supprimer le token OAuth ou le token simple selon le cas.
                if self.auth.has_oauth(&host) {
                    let _ = self.auth.clear_oauth_token(&host);
                } else {
                    let _ = self.auth.clear_token(&host);
                }
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

        let description = text(
            "Connectez vos comptes de fournisseurs Git pour un acces fluide aux depots.",
        )
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
            Space::with_height(theme.spacing.lg),
            accounts_section,
            Space::with_height(theme.spacing.lg),
            oauth_section,
            Space::with_height(theme.spacing.lg),
            manual_section,
        ]
        .spacing(theme.spacing.xs)
        .width(Length::Fill);

        scrollable(container(content).width(Length::Fill).padding(theme.spacing.md))
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

        let hosts = self.auth.known_hosts();
        let oauth_hosts: Vec<_> = hosts
            .iter()
            .filter(|h| !h.starts_with("oauth:"))
            .collect();

        let mut items = column![].spacing(theme.spacing.xs);

        if oauth_hosts.is_empty() {
            items = items.push(
                text("Aucun compte connecte. Utilisez les boutons ci-dessous pour vous connecter.")
                    .size(typo.body)
                    .color(palette.text_secondary),
            );
        } else {
            for host in &oauth_hosts {
                let has_oauth = self.auth.has_oauth(host);
                let auth_type = if has_oauth { "OAuth" } else { "Token" };

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

                let account_row = row![label, Space::with_width(Length::Fill), disconnect_btn]
                    .spacing(theme.spacing.sm)
                    .align_y(Alignment::Center);

                items = items.push(account_row);
            }
        }

        column![title, subtitle, Space::with_height(theme.spacing.sm), items]
            .spacing(theme.spacing.xs)
            .into()
    }

    /// Construit la section des boutons de connexion OAuth.
    fn view_oauth_section<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let title = text("Connexion OAuth")
            .size(typo.title)
            .color(palette.text_primary);

        let description = text(
            "Cliquez pour vous authentifier via votre fournisseur Git. Une fenetre de navigateur s'ouvrira.",
        )
        .size(typo.body)
        .color(palette.text_secondary);

        // Bouton GitHub OAuth.
        let github_available = !self.github_in_progress
            && (GITHUB_CLIENT_ID.is_some() || !self.custom_github_client_id.is_empty());

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
            && (GITLAB_CLIENT_ID.is_some() || !self.custom_gitlab_client_id.is_empty());

        let mut gitlab_btn = button(
            text("Connexion GitLab")
                .size(typo.body)
                .color(palette.text_primary),
        );
        if gitlab_available {
            gitlab_btn = gitlab_btn.on_press(Message::StartGitlabOAuth);
        }

        let oauth_buttons = row![github_btn, Space::with_width(theme.spacing.md), gitlab_btn]
            .align_y(Alignment::Center);

        // Messages de statut.
        let mut status_col = column![].spacing(theme.spacing.xs);
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
            Space::with_height(theme.spacing.sm),
            oauth_buttons,
            status_col,
            Space::with_height(theme.spacing.sm),
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
             URL de callback : http://127.0.0.1:9876/callback",
        )
        .size(typo.label)
        .color(palette.text_secondary);

        let github_label = text("GitHub Client ID :")
            .size(typo.label)
            .color(palette.text_secondary);

        let github_input = text_input(
            "Identifiant OAuth GitHub",
            &self.custom_github_client_id,
        )
        .on_input(Message::GithubClientIdChanged)
        .width(Length::Fixed(280.0));

        let github_row = row![github_label, github_input]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        let gitlab_label = text("GitLab Client ID :")
            .size(typo.label)
            .color(palette.text_secondary);

        let gitlab_input = text_input(
            "Identifiant OAuth GitLab",
            &self.custom_gitlab_client_id,
        )
        .on_input(Message::GitlabClientIdChanged)
        .width(Length::Fixed(280.0));

        let gitlab_row = row![gitlab_label, gitlab_input]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        // Note sur les variables d'environnement.
        let mut col = column![config_hint, Space::with_height(theme.spacing.xs), github_row, gitlab_row]
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
        let host_label = text("Hote :")
            .size(typo.body)
            .color(palette.text_secondary);

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

        let token_input =
            text_input("Jeton d'acces personnel", &self.manual_token)
                .on_input(Message::ManualTokenChanged)
                .secure(true)
                .width(Length::Fixed(300.0));

        let token_row = row![token_label, token_input]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        // Bouton de validation.
        let can_save =
            !self.manual_host.trim().is_empty()
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
            Space::with_height(theme.spacing.sm),
            host_row,
            token_row,
            Space::with_height(theme.spacing.xs),
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
