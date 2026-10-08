// Panneau d'historique des commits pour l'interface Iced.
// Affiche la liste des commits avec filtres, et un volet de details
// avec les fichiers modifies et les diffs expansibles.

use std::collections::HashMap;

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use iced::widget::{
    button, column, container, horizontal_rule, row, scrollable, text, text_input, Space,
};
use iced::{Element, Length};

use crate::ui::theme::Theme;

use crate::git::{
    diff::{FileDiff, FileDiffSummary, commit_diff_file, commit_diff_summaries},
    log::{CommitFilter, CommitInfo, list_local_branches, read_commit_log},
};
use crate::ui::context::RepoContext;

/// Nombre maximal de commits charges au total.
const MAX_COMMITS: usize = 200;

/// Nombre de commits charges par page (chargement incremental).
const PAGE_SIZE: usize = 50;

// ---------------------------------------------------------------------------
// Filtres
// ---------------------------------------------------------------------------

/// Criteres de filtrage de l'historique de commits.
#[derive(Default, Clone)]
pub struct HistoryFilters {
    pub branch: String,
    pub author: String,
    pub search: String,
    pub since: String,
    pub until: String,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Actions possibles depuis le panneau d'historique.
#[derive(Debug, Clone)]
pub enum Message {
    /// Filtre de branche modifie.
    BranchFilterChanged(String),
    /// Filtre d'auteur modifie.
    AuthorChanged(String),
    /// Recherche textuelle modifiee.
    SearchChanged(String),
    /// Application differee de la recherche apres debounce (version, texte).
    ApplyDebouncedSearch(u32),
    /// Date de debut modifiee (format YYYY-MM-DD).
    SinceChanged(String),
    /// Date de fin modifiee (format YYYY-MM-DD).
    UntilChanged(String),
    /// Appliquer les filtres et recharger les commits.
    ApplyFilters,
    /// Selectionner un commit par son identifiant.
    SelectCommit(String),
    /// Etendre ou charger le diff d'un fichier.
    ExpandFile(String),
    /// Charger la page suivante de commits.
    LoadMore,
}

// ---------------------------------------------------------------------------
// Panneau principal
// ---------------------------------------------------------------------------

/// Panneau d'historique affichant les commits et leurs details.
pub struct HistoryPanel {
    filters: HistoryFilters,
    branches: Vec<String>,
    commits: Vec<CommitInfo>,
    selected_commit: Option<String>,
    /// Resumes legers des fichiers modifies (charges a la selection du commit).
    diff_summaries: Vec<FileDiffSummary>,
    /// Diffs complets charges a la demande (cle = chemin du fichier).
    loaded_patches: HashMap<String, FileDiff>,
    /// Ensemble des fichiers dont le diff est actuellement affiche.
    expanded_files: std::collections::HashSet<String>,
    last_repo: Option<String>,
    error: Option<String>,
    diff_error: Option<String>,
    pending_refresh: bool,
    /// Compteur de version pour le debounce de la recherche.
    search_debounce_version: u32,
    /// Nombre de commits actuellement charges (pour la pagination).
    loaded_count: usize,
    /// Indique s'il reste des commits a charger.
    has_more: bool,
}

impl HistoryPanel {
    /// Cree un nouveau panneau d'historique.
    pub fn new() -> Self {
        Self {
            filters: HistoryFilters::default(),
            branches: Vec::new(),
            commits: Vec::new(),
            selected_commit: None,
            diff_summaries: Vec::new(),
            loaded_patches: HashMap::new(),
            expanded_files: std::collections::HashSet::new(),
            last_repo: None,
            error: None,
            diff_error: None,
            pending_refresh: false,
            search_debounce_version: 0,
            loaded_count: 0,
            has_more: false,
        }
    }

    /// Positionne le filtre de branche et declenche un rafraichissement.
    pub fn set_branch_filter(&mut self, branch: String, repo: Option<&RepoContext>) {
        self.filters.branch = branch;
        self.selected_commit = None;
        if let Some(repo) = repo {
            self.refresh(repo);
            self.pending_refresh = false;
        } else {
            self.pending_refresh = true;
        }
    }

    // -----------------------------------------------------------------------
    // Mise a jour (logique)
    // -----------------------------------------------------------------------

    /// Traite un message et effectue les operations correspondantes.
    pub fn update(
        &mut self,
        message: Message,
        repo: Option<&RepoContext>,
    ) -> iced::Task<Message> {
        match message {
            Message::BranchFilterChanged(value) => {
                self.filters.branch = value;
            }

            Message::AuthorChanged(value) => {
                self.filters.author = value;
            }

            Message::SearchChanged(value) => {
                // Stocker le texte immediatement pour l'affichage du champ
                self.filters.search = value;
                // Incrementer la version pour invalider les debounces precedents
                self.search_debounce_version += 1;
                let version = self.search_debounce_version;
                // Retourner une tache qui attend 250ms puis envoie ApplyDebouncedSearch
                return iced::Task::perform(
                    async move {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    },
                    move |_| Message::ApplyDebouncedSearch(version),
                );
            }

            Message::SinceChanged(value) => {
                self.filters.since = value;
            }

            Message::UntilChanged(value) => {
                self.filters.until = value;
            }

            Message::ApplyDebouncedSearch(version) => {
                // Appliquer uniquement si aucune frappe plus recente n'a eu lieu
                if version == self.search_debounce_version {
                    if let Some(repo) = repo {
                        self.refresh(repo);
                    }
                }
            }

            Message::ApplyFilters => {
                if let Some(repo) = repo {
                    self.refresh(repo);
                }
            }

            Message::SelectCommit(id) => {
                self.selected_commit = Some(id);
                self.expanded_files.clear();
                self.load_diff();
            }

            Message::LoadMore => {
                // Charger la page suivante de commits
                if self.has_more {
                    if let Some(repo) = repo {
                        self.load_next_page(repo);
                    }
                }
            }

            Message::ExpandFile(path) => {
                // Basculer l'etat d'expansion du fichier
                if self.expanded_files.contains(&path) {
                    self.expanded_files.remove(&path);
                } else {
                    // Charger le patch si necessaire
                    if !self.loaded_patches.contains_key(&path) {
                        self.load_file_patch(&path);
                    }
                    self.expanded_files.insert(path);
                }
            }
        }

        iced::Task::none()
    }

    // -----------------------------------------------------------------------
    // Vue (rendu)
    // -----------------------------------------------------------------------

    /// Construit l'arbre de widgets Iced pour le panneau d'historique.
    pub fn view<'a>(
        &'a self,
        _theme: &'a Theme,
        repo: Option<&'a RepoContext>,
    ) -> Element<'a, Message> {
        let mut main_col = column![].spacing(8).padding(8);

        // Titre et description
        main_col = main_col
            .push(text("Commit history").size(24))
            .push(text(
                "Explore commits, filter by branch or author, and inspect diffs.",
            ).size(14));

        // Sans depot, message d'aide
        let Some(_repo) = repo else {
            main_col = main_col.push(
                text("Select or clone a repository to view its commit history.").size(14),
            );
            return main_col.into();
        };

        // Erreur globale
        if let Some(error) = &self.error {
            main_col = main_col.push(text(error.as_str()).size(14));
            return main_col.into();
        }

        // Section des filtres
        main_col = main_col.push(self.view_filters());

        main_col = main_col.push(horizontal_rule(1));

        // Deux colonnes : liste des commits | volet de details
        let commit_list = self.view_commit_list();
        let details = self.view_details_pane();

        let two_columns = row![
            container(commit_list).width(Length::FillPortion(55)),
            container(details).width(Length::FillPortion(45)),
        ]
        .spacing(16);

        main_col = main_col.push(two_columns);

        main_col.into()
    }

    /// Section de filtres dans un conteneur stylise.
    fn view_filters(&self) -> Element<'_, Message> {
        // Premiere ligne : branche et auteur
        let branch_input = text_input("All branches", &self.filters.branch)
            .on_input(Message::BranchFilterChanged);
        let author_input = text_input("name or email", &self.filters.author)
            .on_input(Message::AuthorChanged);

        let first_row = row![
            text("Branch").size(14),
            branch_input,
            Space::with_width(Length::Fixed(8.0)),
            text("Author").size(14),
            author_input,
        ]
        .spacing(8);

        // Deuxieme ligne : recherche, dates, bouton appliquer
        let search_input = text_input("message contains", &self.filters.search)
            .on_input(Message::SearchChanged);
        let since_input = text_input("YYYY-MM-DD", &self.filters.since)
            .on_input(Message::SinceChanged);
        let until_input = text_input("YYYY-MM-DD", &self.filters.until)
            .on_input(Message::UntilChanged);
        let apply_btn = button(text("Apply filters")).on_press(Message::ApplyFilters);

        let second_row = row![
            text("Search").size(14),
            search_input,
            Space::with_width(Length::Fixed(8.0)),
            text("Since").size(14),
            since_input,
            text("Until").size(14),
            until_input,
            apply_btn,
        ]
        .spacing(8);

        container(
            column![
                text("Filters").size(20),
                first_row,
                second_row,
            ]
            .spacing(6),
        )
        .padding(10)
        .into()
    }

    /// Liste defilable des commits.
    fn view_commit_list(&self) -> Element<'_, Message> {
        let mut col = column![].spacing(4);

        if self.commits.is_empty() {
            col = col.push(text("No commits match the current filters.").size(14));
            return scrollable(col).into();
        }

        for commit in &self.commits {
            let is_selected = self
                .selected_commit
                .as_deref()
                .map(|id| id == commit.id)
                .unwrap_or(false);

            // Formater la date
            let date_str = chrono::DateTime::<Utc>::from_timestamp(commit.time.seconds(), 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "Unknown time".to_string());

            let commit_id = commit.id.clone();

            // Contenu de la ligne
            let row_content = column![
                row![
                    text(&commit.summary).size(14),
                    Space::with_width(Length::Fixed(8.0)),
                    text(&commit.short_id).size(12),
                ]
                .spacing(4),
                text(format!("{}", commit.author)).size(12),
                text(date_str).size(12),
            ]
            .spacing(2);

            // Envelopper dans un bouton pour la selection
            let commit_btn = button(row_content)
                .on_press(Message::SelectCommit(commit_id))
                .padding(8)
                .width(Length::Fill);

            // Marquer visuellement la selection
            if is_selected {
                col = col.push(
                    container(commit_btn).padding(2),
                );
            } else {
                col = col.push(commit_btn);
            }
        }

        // Bouton pour charger la page suivante si des commits restent disponibles
        if self.has_more {
            col = col.push(
                button(text("Charger plus...").size(14))
                    .on_press(Message::LoadMore)
                    .padding(8)
                    .width(Length::Fill),
            );
        }

        scrollable(col).into()
    }

    /// Volet de details du commit selectionne.
    fn view_details_pane(&self) -> Element<'_, Message> {
        let mut col = column![].spacing(6);
        col = col.push(text("Details").size(20));

        let Some(commit_id) = &self.selected_commit else {
            col = col.push(
                text("Select a commit from the list to see its details and diff.").size(14),
            );
            return col.into();
        };

        // Trouver le commit dans la liste
        let Some(commit) = self.commits.iter().find(|c| &c.id == commit_id) else {
            col = col.push(text("Commit not found.").size(14));
            return col.into();
        };

        // Resume du commit
        col = col.push(text(&commit.summary).size(16));

        // Corps du message (si different du resume)
        let full_message = commit.message.trim();
        let summary_trimmed = commit.summary.trim();
        let message_body = if full_message.starts_with(summary_trimmed) {
            full_message
                .get(summary_trimmed.len()..)
                .unwrap_or("")
                .trim_start()
        } else {
            full_message
        };
        if !message_body.is_empty() {
            col = col.push(text(message_body).size(13));
        }

        // Email de l'auteur
        if let Some(email) = &commit.email {
            col = col.push(text(email.as_str()).size(12));
        }

        // Statistiques du commit
        if let (Some(files), Some(additions), Some(deletions)) =
            (commit.files_changed, commit.additions, commit.deletions)
        {
            col = col.push(
                text(format!(
                    "Files changed: {files} (+{additions}, -{deletions})"
                ))
                .size(12),
            );
        }

        col = col.push(horizontal_rule(1));
        col = col.push(text("Files changed").size(18));

        // Erreur de diff
        if let Some(error) = &self.diff_error {
            col = col.push(text(error.as_str()).size(14));
        }

        // Liste des fichiers modifies avec diffs expansibles
        if self.diff_summaries.is_empty() {
            col = col.push(text("No file changes found.").size(14));
            return scrollable(col).into();
        }

        let mut files_col = column![].spacing(4);
        for summary in &self.diff_summaries {
            let file_path = summary.path.clone();
            let is_expanded = self.expanded_files.contains(&file_path);

            // En-tete du fichier
            let header_text = if summary.is_binary {
                format!("{} (binary)", summary.path)
            } else {
                format!(
                    "{} (+{}, -{})",
                    summary.path, summary.additions, summary.deletions
                )
            };

            let expand_label = if is_expanded { "[-]" } else { "[+]" };

            let header_row = row![
                button(text(expand_label).size(12))
                    .on_press(Message::ExpandFile(file_path.clone()))
                    .padding(2),
                text(header_text).size(13),
            ]
            .spacing(4);

            files_col = files_col.push(header_row);

            // Afficher le diff si le fichier est etendu
            if is_expanded {
                if let Some(diff) = self.loaded_patches.get(&file_path) {
                    if diff.truncated {
                        files_col = files_col.push(
                            text("Fichier tronque (trop volumineux)").size(11),
                        );
                    }
                    // Afficher le patch dans un conteneur a largeur fixe
                    files_col = files_col.push(
                        container(
                            scrollable(
                                text(&diff.patch).size(12),
                            ),
                        )
                        .padding(4)
                        .width(Length::Fill),
                    );
                } else {
                    files_col = files_col.push(text("Loading...").size(12));
                }
            }
        }

        col = col.push(scrollable(files_col));

        scrollable(col).into()
    }

    // -----------------------------------------------------------------------
    // Logique interne
    // -----------------------------------------------------------------------

    /// Recharge la liste des commits selon les filtres courants.
    fn refresh(&mut self, repo: &RepoContext) {
        self.error = None;
        self.diff_error = None;
        self.last_repo = Some(repo.path().to_string());
        self.selected_commit = None;
        self.diff_summaries.clear();
        self.loaded_patches.clear();
        self.expanded_files.clear();

        // Charger les branches locales si necessaire
        if self.branches.is_empty() {
            if let Ok(branches) = list_local_branches(repo.path()) {
                self.branches = branches;
            }
        }

        // Construire le filtre git
        let filter = CommitFilter {
            branch: if self.filters.branch.is_empty() {
                None
            } else {
                Some(self.filters.branch.clone())
            },
            author: if self.filters.author.is_empty() {
                None
            } else {
                Some(self.filters.author.clone())
            },
            search: if self.filters.search.is_empty() {
                None
            } else {
                Some(self.filters.search.clone())
            },
            since: parse_date(&self.filters.since),
            until: parse_date(&self.filters.until),
        };

        // Chargement initial : une seule page
        match read_commit_log(repo.path(), &filter, PAGE_SIZE, false) {
            Ok(commits) => {
                let count = commits.len();
                self.commits = commits;
                self.loaded_count = count;
                // Il reste des commits si la page est pleine et qu'on n'a pas atteint le max
                self.has_more = (count == PAGE_SIZE) && (self.loaded_count < MAX_COMMITS);
            }
            Err(err) => {
                self.commits.clear();
                self.loaded_count = 0;
                self.has_more = false;
                self.error = Some(format!("Echec de lecture des commits : {err}"));
            }
        }
    }

    /// Charge la page suivante de commits en augmentant la limite.
    fn load_next_page(&mut self, repo: &RepoContext) {
        let new_limit = (self.loaded_count + PAGE_SIZE).min(MAX_COMMITS);

        // Reconstruire le filtre identique a celui de refresh()
        let filter = CommitFilter {
            branch: if self.filters.branch.is_empty() {
                None
            } else {
                Some(self.filters.branch.clone())
            },
            author: if self.filters.author.is_empty() {
                None
            } else {
                Some(self.filters.author.clone())
            },
            search: if self.filters.search.is_empty() {
                None
            } else {
                Some(self.filters.search.clone())
            },
            since: parse_date(&self.filters.since),
            until: parse_date(&self.filters.until),
        };

        match read_commit_log(repo.path(), &filter, new_limit, false) {
            Ok(commits) => {
                let count = commits.len();
                self.commits = commits;
                self.loaded_count = count;
                // Verifier s'il reste des commits a charger
                self.has_more = (count == new_limit) && (self.loaded_count < MAX_COMMITS);
            }
            Err(err) => {
                self.error = Some(format!("Echec de lecture des commits : {err}"));
                self.has_more = false;
            }
        }
    }

    /// Charge les resumes de diff pour le commit selectionne.
    fn load_diff(&mut self) {
        self.loaded_patches.clear();
        self.expanded_files.clear();

        let Some(repo) = self.last_repo.clone() else {
            return;
        };
        let Some(commit_id) = &self.selected_commit else {
            return;
        };

        match commit_diff_summaries(&repo, commit_id) {
            Ok(summaries) => {
                self.diff_summaries = summaries;
                self.diff_error = None;
            }
            Err(err) => {
                self.diff_summaries.clear();
                self.diff_error = Some(format!("Echec du chargement du diff : {err}"));
            }
        }
    }

    /// Charge le patch d'un fichier a la demande (chargement paresseux).
    fn load_file_patch(&mut self, file_path: &str) {
        if self.loaded_patches.contains_key(file_path) {
            return; // Deja charge
        }

        let (Some(repo), Some(commit_id)) = (self.last_repo.as_ref(), self.selected_commit.as_ref()) else {
            return;
        };

        match commit_diff_file(repo, commit_id, file_path) {
            Ok(Some(diff)) => {
                self.loaded_patches.insert(file_path.to_string(), diff);
            }
            Ok(None) => {
                // Fichier absent du diff, inserer un espace reservé
                self.loaded_patches.insert(
                    file_path.to_string(),
                    FileDiff {
                        path: file_path.to_string(),
                        additions: 0,
                        deletions: 0,
                        patch: String::from("(pas de modifications)"),
                        truncated: false,
                    },
                );
            }
            Err(err) => {
                self.loaded_patches.insert(
                    file_path.to_string(),
                    FileDiff {
                        path: file_path.to_string(),
                        additions: 0,
                        deletions: 0,
                        patch: format!("Erreur de chargement du diff : {err}"),
                        truncated: false,
                    },
                );
            }
        }
    }
}

/// Convertit une chaine au format YYYY-MM-DD en timestamp Unix.
fn parse_date(input: &str) -> Option<i64> {
    if input.trim().is_empty() {
        return None;
    }

    NaiveDate::parse_from_str(input.trim(), "%Y-%m-%d")
        .ok()
        .and_then(|date| {
            Utc.with_ymd_and_hms(date.year(), date.month(), date.day(), 0, 0, 0)
                .earliest()
        })
        .map(|dt| dt.timestamp())
}
