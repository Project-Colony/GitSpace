//! Panneau de clonage de dépôts distants via GitHub ou GitLab.
//!
//! Ce module gère la recherche de dépôts distants, la sélection et le clonage
//! vers un répertoire local. Les fonctions réseau sont exécutées de manière
//! asynchrone via des tâches Iced.

use std::collections::HashSet;
use std::path::PathBuf;
use std::str::FromStr;

use iced::widget::{button, column, container, pick_list, progress_bar, row, text, text_input, Space};
use iced::{Alignment, Element, Length, Task};
use reqwest::StatusCode;
use reqwest::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT};
use serde::Deserialize;
use url::Url;

use crate::auth::AuthManager;
use crate::config::NetworkOptions;
use crate::error::AppError;
use crate::git::clone::{CloneProgress, CloneRequest, clone_repository};
use crate::ui::theme::Theme;

// ---------------------------------------------------------------------------
// Fournisseur distant (GitHub, GitLab)
// ---------------------------------------------------------------------------

/// Représente un fournisseur de dépôts distants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    GitHub,
    GitLab,
}

impl Provider {
    /// Libellé lisible du fournisseur.
    fn label(&self) -> &'static str {
        match self {
            Provider::GitHub => "GitHub",
            Provider::GitLab => "GitLab",
        }
    }

    /// Hôte principal du fournisseur.
    fn host(&self) -> &'static str {
        match self {
            Provider::GitHub => "github.com",
            Provider::GitLab => "gitlab.com",
        }
    }

    /// Icône Nerd Font du fournisseur.
    fn icon(&self) -> char {
        match self {
            Provider::GitHub => '\u{f408}',
            Provider::GitLab => '\u{f296}',
        }
    }

    /// Icône suivie du libellé, pour l'affichage.
    fn icon_label(&self) -> String {
        format!("{} {}", self.icon(), self.label())
    }
}

/// Nécessaire pour le widget `pick_list` d'Iced.
impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

// ---------------------------------------------------------------------------
// Dépôt distant
// ---------------------------------------------------------------------------

/// Un dépôt trouvé via la recherche distante.
#[derive(Debug, Clone)]
pub struct RemoteRepo {
    pub name: String,
    pub url: String,
}

/// Nécessaire pour le widget `pick_list` — affiche le nom du dépôt.
impl std::fmt::Display for RemoteRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// Comparaison par nom pour la sélection dans `pick_list`.
impl PartialEq for RemoteRepo {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.url == other.url
    }
}

impl Eq for RemoteRepo {}

// ---------------------------------------------------------------------------
// Messages Iced
// ---------------------------------------------------------------------------

/// Messages émis par le panneau de clonage.
#[derive(Debug, Clone)]
pub enum Message {
    /// Le fournisseur sélectionné a changé.
    ProviderSelected(Provider),
    /// Le texte de recherche a changé.
    QueryChanged(String),
    /// L'URL du dépôt a changé.
    UrlChanged(String),
    /// Le chemin de destination a changé.
    DestinationChanged(String),
    /// Lancer la recherche de dépôts.
    Search,
    /// Résultat de la recherche asynchrone.
    SearchResult(Result<Vec<RemoteRepo>, AppError>),
    /// Un dépôt a été sélectionné dans la liste.
    SelectRepo(usize),
    /// Ouvrir le sélecteur de dossier natif.
    ChooseDestination,
    /// Résultat du sélecteur de dossier.
    ChooseDestinationResult(Option<String>),
    /// Démarrer le clonage.
    StartClone,
    /// Progression du clonage (réservé pour usage futur).
    CloneProgress(CloneProgress),
    /// Résultat du clonage asynchrone.
    CloneResult(Result<(), AppError>),
    /// Relancer le dernier clonage échoué.
    RetryClone,
    /// Recherche différée après un délai de saisie.
    DebouncedSearch(u32),
}

// ---------------------------------------------------------------------------
// Panneau de clonage
// ---------------------------------------------------------------------------

/// Panneau principal de clonage de dépôts.
pub struct ClonePanel {
    /// Fournisseur actuellement sélectionné.
    provider: Provider,
    /// Requête de recherche saisie par l'utilisateur.
    repo_query: String,
    /// URL du dépôt à cloner.
    repo_url: String,
    /// Chemin de destination pour le clonage.
    destination: String,
    /// Répertoire de base pour construire les destinations suggérées.
    base_destination: PathBuf,
    /// Résultats de la dernière recherche.
    search_results: Vec<RemoteRepo>,
    /// Index du dépôt sélectionné dans `search_results`.
    selected_repo: Option<usize>,
    /// Message d'état de la recherche affiché à l'utilisateur.
    search_status: Option<String>,
    /// Progression actuelle du clonage.
    progress: Option<CloneProgress>,
    /// Message d'état du clonage affiché à l'utilisateur.
    clone_status: Option<String>,
    /// Indique si un clonage est en cours.
    cloning: bool,
    /// Destination active pendant le clonage.
    active_destination: Option<PathBuf>,
    /// Dernière requête de clonage (pour le retry).
    last_request: Option<CloneRequest>,
    /// Options réseau courantes.
    network: NetworkOptions,
    /// Compteur de version pour le anti-rebond de la recherche.
    search_debounce: u32,
}

impl ClonePanel {
    /// Crée un nouveau panneau de clonage avec la destination et les options réseau.
    pub fn new(destination: String, network: NetworkOptions) -> Self {
        Self {
            provider: Provider::GitHub,
            repo_query: String::new(),
            repo_url: String::new(),
            base_destination: PathBuf::from(&destination),
            destination,
            search_results: Vec::new(),
            selected_repo: None,
            search_status: None,
            progress: None,
            clone_status: None,
            cloning: false,
            active_destination: None,
            last_request: None,
            network,
            search_debounce: 0,
        }
    }

    /// Met à jour le chemin de destination par défaut.
    pub fn set_default_destination<S: Into<String>>(&mut self, destination: S) {
        self.destination = destination.into();
        self.base_destination = PathBuf::from(self.destination.clone());
    }

    /// Met à jour les préférences réseau.
    pub fn set_network_preferences(&mut self, network: NetworkOptions) {
        self.network = network;
    }

    /// Traite un message et renvoie une tâche Iced plus un éventuel chemin cloné.
    ///
    /// Le `Option<PathBuf>` signale au parent qu'un dépôt a été cloné avec succès.
    pub fn update(
        &mut self,
        message: Message,
        auth: &AuthManager,
    ) -> (Task<Message>, Option<PathBuf>) {
        match message {
            Message::ProviderSelected(provider) => {
                self.provider = provider;
                self.search_results.clear();
                self.selected_repo = None;
                (Task::none(), None)
            }

            Message::QueryChanged(query) => {
                self.repo_query = query;
                self.search_status = None;
                // Incrémenter le compteur et programmer une recherche différée (250 ms)
                self.search_debounce += 1;
                let version = self.search_debounce;
                let task = Task::perform(
                    async move {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                        version
                    },
                    Message::DebouncedSearch,
                );
                (task, None)
            }

            Message::UrlChanged(url) => {
                self.repo_url = url;
                (Task::none(), None)
            }

            Message::DestinationChanged(dest) => {
                self.destination = dest;
                (Task::none(), None)
            }

            Message::Search => {
                let query = self.repo_query.trim().to_string();
                if query.len() < 2 {
                    return (Task::none(), None);
                }
                let provider = self.provider;
                let token = self.resolve_search_token(auth);
                let network = self.network.clone();
                self.search_status = Some("Recherche en cours...".into());

                let task = Task::perform(
                    async move {
                        search_repositories(provider, &query, token.as_deref(), network).await
                    },
                    |result| Message::SearchResult(result),
                );
                (task, None)
            }

            Message::SearchResult(result) => {
                match result {
                    Ok(results) => {
                        let count = results.len();
                        self.search_results = results;
                        self.search_status = Some(format!("{count} résultat(s)"));
                        // Sélectionner automatiquement le premier résultat
                        if let Some(repo) = self.search_results.first().cloned() {
                            self.selected_repo = Some(0);
                            self.update_selection(&repo);
                        }
                    }
                    Err(err) => {
                        self.search_status = Some(err.user_message());
                    }
                }
                (Task::none(), None)
            }

            Message::SelectRepo(index) => {
                if let Some(repo) = self.search_results.get(index).cloned() {
                    self.selected_repo = Some(index);
                    self.update_selection(&repo);
                }
                (Task::none(), None)
            }

            Message::ChooseDestination => {
                let current = self.destination.clone();
                let task = Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            rfd::FileDialog::new()
                                .set_directory(&current)
                                .pick_folder()
                                .map(|p| p.display().to_string())
                        })
                        .await
                    },
                    |join_result| match join_result {
                        Ok(result) => Message::ChooseDestinationResult(result),
                        Err(_) => Message::ChooseDestinationResult(None),
                    },
                );
                (task, None)
            }

            Message::ChooseDestinationResult(path) => {
                if let Some(folder) = path {
                    self.base_destination = PathBuf::from(&folder);
                    self.destination = folder;
                }
                (Task::none(), None)
            }

            Message::StartClone => {
                let url = self.repo_url.trim().to_string();
                let destination = PathBuf::from(self.destination.trim());
                let token = auth.resolve_for_url(&url);
                let request = CloneRequest {
                    url,
                    destination,
                    token,
                    network: self.network.clone(),
                };
                let task = self.begin_clone(request);
                (task, None)
            }

            Message::CloneProgress(progress) => {
                self.progress = Some(progress);
                (Task::none(), None)
            }

            Message::CloneResult(result) => {
                self.cloning = false;
                match result {
                    Ok(()) => {
                        self.clone_status =
                            Some("Clonage terminé avec succès".into());
                        let cloned_path = self.active_destination.take();
                        (Task::none(), cloned_path)
                    }
                    Err(err) => {
                        self.clone_status = Some(err.user_message());
                        self.active_destination = None;
                        (Task::none(), None)
                    }
                }
            }

            Message::RetryClone => {
                if self.cloning {
                    return (Task::none(), None);
                }
                if let Some(request) = self.last_request.clone() {
                    let task = self.begin_clone(request);
                    (task, None)
                } else {
                    (Task::none(), None)
                }
            }

            Message::DebouncedSearch(version) => {
                // Ignorer si une frappe plus récente a incrémenté le compteur
                if version != self.search_debounce {
                    return (Task::none(), None);
                }
                // Même logique que Message::Search
                let query = self.repo_query.trim().to_string();
                if query.len() < 2 {
                    return (Task::none(), None);
                }
                let provider = self.provider;
                let token = self.resolve_search_token(auth);
                let network = self.network.clone();
                self.search_status = Some("Recherche en cours...".into());

                let task = Task::perform(
                    async move {
                        search_repositories(provider, &query, token.as_deref(), network).await
                    },
                    |result| Message::SearchResult(result),
                );
                (task, None)
            }
        }
    }

    /// Lance le clonage en arrière-plan et renvoie la tâche Iced correspondante.
    fn begin_clone(&mut self, request: CloneRequest) -> Task<Message> {
        self.last_request = Some(request.clone());
        self.active_destination = Some(request.destination.clone());
        self.progress = None;
        self.clone_status = Some("Démarrage du clonage...".into());
        self.cloning = true;

        // Le clonage est bloquant — on le lance dans un thread dédié.
        // La progression en temps réel n'est pas disponible dans cette version ;
        // le résultat final est renvoyé une fois le clonage terminé.
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    clone_repository(request, |_progress| {
                        // Progression ignorée pour l'instant — pas de canal temps réel.
                    })
                })
                .await
            },
            |join_result| match join_result {
                Ok(result) => Message::CloneResult(result),
                Err(err) => Message::CloneResult(Err(AppError::Network(
                    format!("Tâche de clonage interrompue : {err}"),
                ))),
            },
        )
    }

    /// Met à jour l'URL et la destination d'après le dépôt sélectionné.
    fn update_selection(&mut self, repo: &RemoteRepo) {
        self.repo_url = repo.url.clone();
        if let Some(destination) = self.suggested_destination(&repo.url) {
            self.destination = destination;
        }
    }

    /// Propose un chemin de destination basé sur le nom du dépôt extrait de l'URL.
    fn suggested_destination(&self, repo_url: &str) -> Option<String> {
        let repo_name = repo_name_from_url(repo_url)?;
        let base = if self.base_destination.as_os_str().is_empty() {
            PathBuf::from(self.destination.clone())
        } else {
            self.base_destination.clone()
        };
        Some(base.join(repo_name).display().to_string())
    }

    /// Résout le jeton d'authentification pour la recherche sur le fournisseur actuel.
    fn resolve_search_token(&self, auth: &AuthManager) -> Option<String> {
        match self.provider {
            Provider::GitHub => auth
                .resolve_for_host(self.provider.host())
                .or_else(|| auth.resolve_for_host("api.github.com")),
            Provider::GitLab => auth.resolve_for_host(self.provider.host()),
        }
    }

    // -----------------------------------------------------------------------
    // Vue Iced
    // -----------------------------------------------------------------------

    /// Construit l'arbre de widgets du panneau de clonage.
    pub fn view<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let title = text("Cloner un dépôt")
            .size(theme.typography.title)
            .color(theme.palette.text_primary);

        let subtitle = text(
            "Choisissez un fournisseur, recherchez un dépôt distant, \
             ou collez une URL pour cloner vers un chemin local.",
        )
        .size(theme.typography.body)
        .color(theme.palette.text_secondary);

        // --- Cartes de fournisseur ---
        let provider_row = self.view_provider_cards(theme);

        // --- Section de recherche ---
        let search_section = self.view_search_section(theme);

        // --- URL du dépôt ---
        let url_section = self.view_url_section(theme);

        // --- Section de destination ---
        let destination_section = self.view_destination_section(theme);

        // --- Barre d'actions ---
        let action_bar = self.view_action_bar(theme);

        // --- Progression ---
        let progress_section = self.view_progress_section(theme);

        // --- Statut du clonage ---
        let status_section: Element<'a, Message> = if let Some(status) = &self.clone_status {
            text(status.as_str())
                .size(theme.typography.body)
                .color(theme.palette.text_secondary)
                .into()
        } else {
            Space::new(0, 0).into()
        };

        let content = column![
            title,
            subtitle,
            Space::with_height(12),
            provider_row,
            Space::with_height(8),
            search_section,
            Space::with_height(12),
            url_section,
            Space::with_height(12),
            destination_section,
            Space::with_height(12),
            action_bar,
            progress_section,
            status_section,
        ]
        .spacing(4)
        .width(Length::Fill);

        container(content)
            .padding(16)
            .width(Length::Fill)
            .into()
    }

    /// Affiche les cartes de sélection du fournisseur (GitHub / GitLab).
    fn view_provider_cards<'a>(&self, theme: &'a Theme) -> Element<'a, Message> {
        let cards: Vec<Element<'a, Message>> = [Provider::GitHub, Provider::GitLab]
            .iter()
            .map(|&provider| {
                let is_active = self.provider == provider;
                let bg = if is_active {
                    theme.palette.surface_highlight
                } else {
                    theme.palette.surface
                };
                let label = text(provider.icon_label())
                    .size(theme.typography.title)
                    .color(theme.palette.text_primary);

                button(
                    container(label)
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                        .width(140)
                        .height(80),
                )
                .style(move |_theme, status| {
                    let mut appearance = button::Style {
                        background: Some(iced::Background::Color(bg)),
                        border: iced::Border {
                            color: if is_active {
                                theme.palette.accent
                            } else {
                                theme.palette.accent_weak
                            },
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        text_color: theme.palette.text_primary,
                        ..button::Style::default()
                    };
                    if matches!(status, button::Status::Hovered) {
                        appearance.background =
                            Some(iced::Background::Color(theme.palette.surface_highlight));
                    }
                    appearance
                })
                .on_press(Message::ProviderSelected(provider))
                .width(140)
                .height(80)
                .into()
            })
            .collect();

        row(cards).spacing(12).into()
    }

    /// Affiche la section de recherche : champ texte + bouton + liste de résultats.
    fn view_search_section<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let heading = text("Recherche de dépôts distants")
            .size(theme.typography.title)
            .color(theme.palette.text_primary);

        let help_text = text(format!(
            "Recherchez sur {} GitHub ou {} GitLab sans quitter l'application.",
            Provider::GitHub.icon(),
            Provider::GitLab.icon(),
        ))
        .size(theme.typography.body)
        .color(theme.palette.text_secondary);

        // Champ de recherche
        let query_input = text_input("Rechercher des dépôts", &self.repo_query)
            .on_input(Message::QueryChanged)
            .width(320);

        // Bouton de recherche — désactivé si la requête est trop courte ou si un clonage est en cours
        let search_enabled = self.repo_query.trim().len() >= 2 && !self.cloning;
        let search_btn = if search_enabled {
            button(text("Rechercher")).on_press(Message::Search)
        } else {
            button(text("Rechercher"))
        };

        // Statut de la recherche
        let status_label: Element<'_, Message> = if let Some(status) = &self.search_status {
            text(status.as_str())
                .size(theme.typography.body)
                .color(theme.palette.text_secondary)
                .into()
        } else {
            Space::new(0, 0).into()
        };

        let search_row = row![query_input, search_btn, status_label]
            .spacing(8)
            .align_y(Alignment::Center);

        // Liste déroulante des résultats — mappe la sélection vers SelectRepo(index)
        let results_section = self.view_results_pick_list(theme);

        column![heading, help_text, Space::with_height(6), search_row, Space::with_height(8), results_section]
            .spacing(2)
            .into()
    }

    /// Widget pick_list pour les résultats de recherche.
    ///
    /// On mappe la sélection vers `SelectRepo(index)` en cherchant l'index par nom.
    fn view_results_pick_list<'a>(&'a self, _theme: &'a Theme) -> Element<'a, Message> {
        let selected = self
            .selected_repo
            .and_then(|idx| self.search_results.get(idx))
            .cloned();

        let results = self.search_results.clone();

        pick_list(results, selected, {
            let search_results = self.search_results.clone();
            move |repo: RemoteRepo| {
                // Trouver l'index correspondant au dépôt sélectionné
                let index = search_results
                    .iter()
                    .position(|r| r.name == repo.name && r.url == repo.url)
                    .unwrap_or(0);
                Message::SelectRepo(index)
            }
        })
        .placeholder("Sélectionnez un dépôt")
        .width(520)
        .into()
    }

    /// Affiche le champ URL du dépôt.
    fn view_url_section<'a>(&self, theme: &'a Theme) -> Element<'a, Message> {
        let label = text("URL du dépôt")
            .size(theme.typography.body)
            .color(theme.palette.text_primary);

        let url_input = text_input(
            "https://github.com/owner/repo.git ou git@gitlab.com:owner/repo.git",
            &self.repo_url,
        )
        .on_input(Message::UrlChanged)
        .width(520);

        column![label, url_input].spacing(4).into()
    }

    /// Affiche la section de destination : champ texte + bouton « Choisir ».
    fn view_destination_section<'a>(&self, theme: &'a Theme) -> Element<'a, Message> {
        let label = text("Chemin local")
            .size(theme.typography.title)
            .color(theme.palette.text_primary);

        let dest_input =
            text_input("Où cloner le dépôt ?", &self.destination)
                .on_input(Message::DestinationChanged)
                .width(400);

        let choose_btn = button(text("Choisir")).on_press(Message::ChooseDestination);

        let dest_row = row![dest_input, choose_btn]
            .spacing(8)
            .align_y(Alignment::Center);

        column![label, dest_row].spacing(4).into()
    }

    /// Affiche le bouton de clonage, aligné à droite.
    fn view_action_bar<'a>(&self, _theme: &'a Theme) -> Element<'a, Message> {
        let can_clone =
            !self.repo_url.trim().is_empty() && !self.destination.trim().is_empty() && !self.cloning;

        let clone_btn = if can_clone {
            button(text("Cloner le dépôt")).on_press(Message::StartClone)
        } else {
            button(text("Cloner le dépôt"))
        };

        // Alignement à droite via un espace flexible
        row![Space::with_width(Length::Fill), clone_btn]
            .width(Length::Fill)
            .into()
    }

    /// Affiche la barre de progression si un clonage est en cours.
    fn view_progress_section<'a>(&self, theme: &'a Theme) -> Element<'a, Message> {
        match &self.progress {
            Some(progress) => {
                let ratio = if progress.total_objects == 0 {
                    0.0
                } else {
                    progress.received_objects as f32 / progress.total_objects as f32
                };

                let label = text("Progression du clonage")
                    .size(theme.typography.body)
                    .color(theme.palette.text_primary);

                let bar = progress_bar(0.0..=1.0, ratio).width(Length::Fill);

                let detail = text(format!(
                    "Objets {}/{} ({:.1} Ko) — Deltas indexés {}/{}",
                    progress.received_objects,
                    progress.total_objects,
                    progress.received_bytes as f64 / 1024.0,
                    progress.indexed_deltas,
                    progress.total_deltas,
                ))
                .size(theme.typography.body)
                .color(theme.palette.text_secondary);

                column![Space::with_height(10), label, bar, detail]
                    .spacing(4)
                    .width(Length::Fill)
                    .into()
            }
            None => Space::new(0, 0).into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Fonctions utilitaires (extraction du nom de dépôt depuis une URL)
// ---------------------------------------------------------------------------

/// Extrait le nom du dépôt depuis une URL (HTTPS ou SSH).
fn repo_name_from_url(repo_url: &str) -> Option<String> {
    let trimmed = repo_url.trim().trim_end_matches('/');
    if let Ok(url) = Url::parse(trimmed) {
        if let Some(segment) = url
            .path_segments()
            .and_then(|segments| segments.filter(|s| !s.is_empty()).last())
        {
            let name = segment.trim_end_matches(".git");
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }

    let mut candidate = trimmed;
    if let Some(idx) = trimmed.rfind(':') {
        candidate = &trimmed[idx + 1..];
    }

    if let Some(idx) = candidate.rfind('/') {
        candidate = &candidate[idx + 1..];
    }

    let name = candidate.trim_end_matches(".git");
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

// ---------------------------------------------------------------------------
// Fonctions réseau — identiques à l'original
// ---------------------------------------------------------------------------

/// Lance la recherche sur le fournisseur approprié.
async fn search_repositories(
    provider: Provider,
    query: &str,
    token: Option<&str>,
    network: NetworkOptions,
) -> Result<Vec<RemoteRepo>, AppError> {
    match provider {
        Provider::GitHub => search_github(query, token, &network).await,
        Provider::GitLab => search_gitlab(query, token, &network).await,
    }
}

/// Construit un client HTTP avec les en-têtes nécessaires (auth, proxy, timeout).
async fn client_with_headers(
    token: Option<&str>,
    token_header: Option<&str>,
    header: Option<(&'static str, &'static str)>,
    network: &NetworkOptions,
) -> Result<Client, AppError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str("gitspace-ui/0.1")
            .map_err(|err| AppError::Validation(err.to_string()))?,
    );
    if let Some(token) = token {
        let header_name = token_header.unwrap_or("Authorization");
        let value = if header_name.eq_ignore_ascii_case("authorization") {
            format!("Bearer {}", token)
        } else {
            token.to_string()
        };
        let name = HeaderName::from_str(header_name)
            .map_err(|err| AppError::Validation(err.to_string()))?;
        let auth_value =
            HeaderValue::from_str(&value).map_err(|err| AppError::Validation(err.to_string()))?;
        headers.insert(name, auth_value);
    }
    if let Some((key, value)) = header {
        headers.insert(
            key,
            HeaderValue::from_str(value).map_err(|err| AppError::Validation(err.to_string()))?,
        );
    }
    let mut builder =
        Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(
                network.network_timeout_secs.max(1),
            ));

    if !network.http_proxy.is_empty() {
        builder = builder.proxy(
            reqwest::Proxy::http(&network.http_proxy)
                .map_err(|err| AppError::Validation(err.to_string()))?,
        );
    }

    if !network.https_proxy.is_empty() {
        builder = builder.proxy(
            reqwest::Proxy::https(&network.https_proxy)
                .map_err(|err| AppError::Validation(err.to_string()))?,
        );
    }

    builder.build().map_err(AppError::from)
}

/// Vérifie que l'URL respecte la politique HTTPS configurée.
fn enforce_https_policy(url: &str, network: &NetworkOptions) -> Result<(), AppError> {
    if url.starts_with("https://") && !network.use_https {
        return Err(AppError::Validation(
            "HTTPS endpoints are disabled in your network settings.".to_string(),
        ));
    }

    if url.starts_with("http://") && network.use_https {
        return Err(AppError::Validation(
            "HTTP requests are blocked. Enable HTTP in network settings or use HTTPS.".to_string(),
        ));
    }

    Ok(())
}

// --- Structures de désérialisation GitHub ---

#[derive(Debug, Deserialize)]
struct GithubRepoOwner {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GithubRepoItem {
    full_name: String,
    html_url: String,
    owner: GithubRepoOwner,
}

#[derive(Debug, Deserialize)]
struct GithubUserProfile {
    login: String,
}

/// Recherche les dépôts d'un compte GitHub (publics + privés si authentifié).
async fn search_github(
    query: &str,
    token: Option<&str>,
    network: &NetworkOptions,
) -> Result<Vec<RemoteRepo>, AppError> {
    let account = query.trim();
    if account.is_empty() {
        return Ok(Vec::new());
    }

    let client = client_with_headers(token, None, None, network).await?;
    let mut unique = HashSet::new();
    let mut repositories = Vec::new();

    let public_user_url = format!("https://api.github.com/users/{}/repos", account);
    repositories.extend(
        fetch_github_repos(&client, &public_user_url, &[("type", "public")], network)
            .await
            .unwrap_or_default(),
    );

    let public_org_url = format!("https://api.github.com/orgs/{}/repos", account);
    repositories.extend(
        fetch_github_repos(&client, &public_org_url, &[("type", "public")], network)
            .await
            .unwrap_or_default(),
    );

    if token.is_some() {
        if let Some(login) = fetch_github_login(&client, network).await? {
            if login.eq_ignore_ascii_case(account) {
                let private_repos = fetch_github_repos(
                    &client,
                    "https://api.github.com/user/repos",
                    &[
                        ("visibility", "all"),
                        ("affiliation", "owner,collaborator,organization_member"),
                    ],
                    network,
                )
                .await?;

                repositories.extend(
                    private_repos
                        .into_iter()
                        .filter(|repo| repo.owner.login.eq_ignore_ascii_case(account)),
                );
            }
        }
    }

    let results = repositories
        .into_iter()
        .filter(|repo| unique.insert(repo.full_name.clone()))
        .map(|item| RemoteRepo {
            name: item.full_name,
            url: item.html_url,
        })
        .collect();

    Ok(results)
}

// --- Structure de désérialisation GitLab ---

#[derive(Debug, Deserialize)]
struct GitlabProject {
    name_with_namespace: String,
    http_url_to_repo: String,
}

/// Recherche les projets GitLab correspondant à la requête.
async fn search_gitlab(
    query: &str,
    token: Option<&str>,
    network: &NetworkOptions,
) -> Result<Vec<RemoteRepo>, AppError> {
    let base_url = "https://gitlab.com/api/v4/projects";
    enforce_https_policy(base_url, network)?;
    let client = client_with_headers(
        token,
        Some("PRIVATE-TOKEN"),
        Some(("Accept", "application/json")),
        network,
    )
    .await?;

    let mut all_projects = Vec::new();
    let per_page = 100;

    for page in 1..=10 {
        let response = client
            .get(base_url)
            .query(&[
                ("search", query),
                ("per_page", &per_page.to_string()),
                ("page", &page.to_string()),
                ("simple", "true"),
            ])
            .send()
            .await
            .map_err(AppError::from)?;

        if !response.status().is_success() {
            if page == 1 {
                // error_for_status() renvoie Err quand le statut n'est pas un succès
                match response.error_for_status() {
                    Err(err) => return Err(AppError::from(err)),
                    Ok(_) => return Err(AppError::Network(
                        "Réponse inattendue du serveur GitLab".to_string(),
                    )),
                }
            }
            break;
        }

        let projects: Vec<GitlabProject> = response.json().await.map_err(AppError::from)?;
        let count = projects.len();
        all_projects.extend(projects);

        if count < per_page {
            break;
        }
    }

    Ok(all_projects
        .into_iter()
        .map(|project| RemoteRepo {
            name: project.name_with_namespace,
            url: project.http_url_to_repo,
        })
        .collect())
}

/// Récupère le login de l'utilisateur GitHub authentifié.
async fn fetch_github_login(
    client: &Client,
    network: &NetworkOptions,
) -> Result<Option<String>, AppError> {
    let url = "https://api.github.com/user";
    enforce_https_policy(url, network)?;
    let response = client.get(url).send().await.map_err(AppError::from)?;
    if response.status() == StatusCode::UNAUTHORIZED {
        return Ok(None);
    }
    let profile: GithubUserProfile = response
        .error_for_status()?
        .json()
        .await
        .map_err(AppError::from)?;
    Ok(Some(profile.login))
}

/// Récupère les dépôts GitHub depuis une URL paginée.
async fn fetch_github_repos(
    client: &Client,
    base_url: &str,
    params: &[(&str, &str)],
    network: &NetworkOptions,
) -> Result<Vec<GithubRepoItem>, AppError> {
    enforce_https_policy(base_url, network)?;
    let mut all_repos = Vec::new();
    let per_page = 100;

    for page in 1..=10 {
        let mut query_params: Vec<(&str, String)> =
            params.iter().map(|(k, v)| (*k, (*v).to_string())).collect();
        query_params.push(("per_page", per_page.to_string()));
        query_params.push(("page", page.to_string()));

        let response = client
            .get(base_url)
            .query(&query_params)
            .send()
            .await
            .map_err(AppError::from)?;

        if response.status() == StatusCode::NOT_FOUND {
            break;
        }

        let response = response.error_for_status().map_err(AppError::from)?;
        let mut repos: Vec<GithubRepoItem> = response.json().await.map_err(AppError::from)?;
        let count = repos.len();
        all_repos.append(&mut repos);

        if count < per_page {
            break;
        }
    }

    Ok(all_repos)
}
