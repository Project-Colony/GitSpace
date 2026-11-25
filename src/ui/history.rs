// Panneau d'historique des commits pour l'interface Iced.
// Affiche la liste des commits avec filtres, et un volet de details
// avec les fichiers modifies et les diffs expansibles.
// Les lectures git tournent en tache de fond ; view() ne lit que l'etat du panneau.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use iced::widget::{
    button, column, container, pick_list, row, rule, scrollable, text, text_input, Space,
};
use iced::{Element, Length, Task};

use crate::ui::theme::Theme;

use crate::git::{
    diff::{commit_diff_file, commit_diff_summaries, FileDiff, FileDiffSummary},
    log::{list_local_branches, read_commit_log, CommitFilter, CommitInfo},
};
use crate::ui::context::git_task;
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

impl HistoryFilters {
    /// Filtre git correspondant aux champs saisis ; un champ vide ne filtre pas.
    fn to_commit_filter(&self) -> CommitFilter {
        let non_empty = |value: &str| (!value.is_empty()).then(|| value.to_string());
        CommitFilter {
            branch: non_empty(&self.branch),
            author: non_empty(&self.author),
            search: non_empty(&self.search),
            since: parse_date(&self.since),
            until: parse_date(&self.until),
        }
    }
}

/// Une page de commits et les branches locales proposees dans le filtre.
#[derive(Debug, Clone)]
pub struct LogPage {
    commits: Vec<CommitInfo>,
    branches: Vec<String>,
    /// Nombre de commits demandes : une page pleine signale qu'il en reste.
    limit: usize,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Actions possibles depuis le panneau d'historique.
#[derive(Debug, Clone)]
pub enum Message {
    /// Filtre de branche modifie.
    BranchFilterChanged(String),
    /// Branche locale choisie dans la liste (appliquee tout de suite).
    BranchPicked(String),
    /// Filtre d'auteur modifie.
    AuthorChanged(String),
    /// Recherche textuelle modifiee.
    SearchChanged(String),
    /// Application differee de la recherche apres debounce (version).
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
    /// Resultat d'une lecture du journal (numero de requete, page).
    Loaded(Arc<str>, u64, Result<LogPage, String>),
    /// Fichiers modifies d'un commit (identifiant, resumes).
    SummariesLoaded(Arc<str>, String, Result<Vec<FileDiffSummary>, String>),
    /// Patch d'un fichier (identifiant du commit, chemin, patch).
    PatchLoaded(Arc<str>, String, String, FileDiff),
}

// ---------------------------------------------------------------------------
// Panneau principal
// ---------------------------------------------------------------------------

/// Panneau d'historique affichant les commits et leurs details.
pub struct HistoryPanel {
    repo: Option<Arc<str>>,
    filters: HistoryFilters,
    /// Filtre de la derniere recherche lancee, reutilise par la pagination.
    applied: CommitFilter,
    /// Branches locales proposees dans le filtre de branche.
    branches: Vec<String>,
    commits: Vec<CommitInfo>,
    selected_commit: Option<String>,
    /// Resumes legers des fichiers modifies (charges a la selection du commit).
    diff_summaries: Vec<FileDiffSummary>,
    /// Diffs complets charges a la demande (cle = chemin du fichier).
    loaded_patches: HashMap<String, FileDiff>,
    /// Ensemble des fichiers dont le diff est actuellement affiche.
    expanded_files: HashSet<String>,
    error: Option<String>,
    diff_error: Option<String>,
    /// Compteur de version pour le debounce de la recherche.
    search_debounce_version: u32,
    /// Numero de la derniere lecture du journal ; les reponses plus anciennes sont ignorees.
    request: u64,
    loading: bool,
    /// Indique s'il reste des commits a charger.
    has_more: bool,
}

impl HistoryPanel {
    /// Cree un nouveau panneau d'historique.
    pub fn new() -> Self {
        Self {
            repo: None,
            filters: HistoryFilters::default(),
            applied: CommitFilter::default(),
            branches: Vec::new(),
            commits: Vec::new(),
            selected_commit: None,
            diff_summaries: Vec::new(),
            loaded_patches: HashMap::new(),
            expanded_files: HashSet::new(),
            error: None,
            diff_error: None,
            search_debounce_version: 0,
            request: 0,
            loading: false,
            has_more: false,
        }
    }

    /// Oublie le depot precedent et charge l'historique de `repo` en tache de fond.
    /// Les filtres saisis sont conserves, sauf la branche quand on change de depot.
    pub fn load(&mut self, repo: &RepoContext) -> Task<Message> {
        let mut filters = std::mem::take(&mut self.filters);
        if self.repo.is_some() && !self.is_current(repo.path()) {
            filters.branch.clear();
        }
        *self = Self {
            repo: Some(repo.path_arc()),
            filters,
            // Keep request numbers growing so a reply from before the reset is ignored.
            request: self.request,
            ..Self::new()
        };
        self.refresh()
    }

    /// Affiche l'historique de `branch` dans `repo`.
    pub fn show_branch(&mut self, branch: String, repo: &RepoContext) -> Task<Message> {
        if !self.is_current(repo.path()) {
            // Only the reset is wanted: a task that is never run never starts its job.
            let _ = self.load(repo);
        }
        self.filters.branch = branch;
        self.refresh()
    }

    // -----------------------------------------------------------------------
    // Mise a jour (logique)
    // -----------------------------------------------------------------------

    /// Traite un message et effectue les operations correspondantes.
    pub fn update(&mut self, message: Message, _repo: Option<&RepoContext>) -> Task<Message> {
        match message {
            Message::BranchFilterChanged(value) => {
                self.filters.branch = value;
            }

            Message::BranchPicked(value) => {
                self.filters.branch = value;
                return self.refresh();
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
                return Task::perform(
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
                    return self.refresh();
                }
            }

            Message::ApplyFilters => {
                return self.refresh();
            }

            Message::SelectCommit(id) => {
                self.selected_commit = Some(id);
                return self.load_diff();
            }

            Message::LoadMore => {
                // La page suivante relit les premiers commits correspondants, plus une page
                if self.has_more && !self.loading {
                    let limit = (self.commits.len() + PAGE_SIZE).min(MAX_COMMITS);
                    return self.fetch(limit);
                }
            }

            Message::ExpandFile(path) => {
                // Basculer l'etat d'expansion du fichier
                if self.expanded_files.remove(&path) {
                    return Task::none();
                }
                self.expanded_files.insert(path.clone());
                if !self.loaded_patches.contains_key(&path) {
                    return self.load_file_patch(path);
                }
            }

            // -- Resultats des taches de fond --
            Message::Loaded(repo, request, result) => {
                if !self.is_current(&repo) || request != self.request {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(page) => {
                        self.has_more =
                            page.commits.len() == page.limit && page.limit < MAX_COMMITS;
                        self.commits = page.commits;
                        self.branches = page.branches;
                    }
                    Err(err) => {
                        self.commits.clear();
                        self.has_more = false;
                        self.error = Some(format!("Echec de lecture des commits : {err}"));
                    }
                }
            }

            Message::SummariesLoaded(repo, id, result) => {
                if self.is_current(&repo) && self.selected_commit.as_ref() == Some(&id) {
                    match result {
                        Ok(summaries) => self.diff_summaries = summaries,
                        Err(err) => {
                            self.diff_error = Some(format!("Echec du chargement du diff : {err}"));
                        }
                    }
                }
            }

            Message::PatchLoaded(repo, id, path, diff) => {
                if self.is_current(&repo) && self.selected_commit.as_ref() == Some(&id) {
                    self.loaded_patches.insert(path, diff);
                }
            }
        }

        Task::none()
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
            .push(text("Explore commits, filter by branch or author, and inspect diffs.").size(14));

        // Sans depot, message d'aide
        if repo.is_none() {
            main_col = main_col
                .push(text("Select or clone a repository to view its commit history.").size(14));
            return main_col.into();
        }

        // Section des filtres
        main_col = main_col.push(self.view_filters());

        // Erreur globale, sous les filtres pour pouvoir les corriger
        if let Some(error) = &self.error {
            main_col = main_col.push(text(error.as_str()).size(14).style(text::danger));
        }

        main_col = main_col.push(rule::horizontal(1));

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
        let branch_input =
            text_input("All branches", &self.filters.branch).on_input(Message::BranchFilterChanged);
        let branch_pick = pick_list(
            self.branches.as_slice(),
            None::<String>,
            Message::BranchPicked,
        )
        .placeholder("Branches locales");
        let author_input =
            text_input("name or email", &self.filters.author).on_input(Message::AuthorChanged);

        let first_row = row![
            text("Branch").size(14),
            branch_input,
            branch_pick,
            Space::new().width(Length::Fixed(8.0)),
            text("Author").size(14),
            author_input,
        ]
        .spacing(8);

        // Deuxieme ligne : recherche, dates, bouton appliquer
        let search_input =
            text_input("message contains", &self.filters.search).on_input(Message::SearchChanged);
        let since_input =
            text_input("YYYY-MM-DD", &self.filters.since).on_input(Message::SinceChanged);
        let until_input =
            text_input("YYYY-MM-DD", &self.filters.until).on_input(Message::UntilChanged);
        let apply_btn = button(text("Apply filters")).on_press(Message::ApplyFilters);

        let second_row = row![
            text("Search").size(14),
            search_input,
            Space::new().width(Length::Fixed(8.0)),
            text("Since").size(14),
            since_input,
            text("Until").size(14),
            until_input,
            apply_btn,
        ]
        .spacing(8);

        container(column![text("Filters").size(20), first_row, second_row,].spacing(6))
            .padding(10)
            .into()
    }

    /// Liste defilable des commits.
    fn view_commit_list(&self) -> Element<'_, Message> {
        let mut col = column![].spacing(4);

        if self.commits.is_empty() {
            let empty = if self.loading {
                "Chargement..."
            } else {
                "No commits match the current filters."
            };
            col = col.push(text(empty).size(14));
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
                    Space::new().width(Length::Fixed(8.0)),
                    text(&commit.short_id).size(12),
                ]
                .spacing(4),
                text(commit.author.to_string()).size(12),
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
                col = col.push(container(commit_btn).padding(2));
            } else {
                col = col.push(commit_btn);
            }
        }

        // Bouton pour charger la page suivante si des commits restent disponibles
        if self.has_more {
            col = col.push(
                button(text("Charger plus...").size(14))
                    .on_press_maybe((!self.loading).then_some(Message::LoadMore))
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
            col = col
                .push(text("Select a commit from the list to see its details and diff.").size(14));
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

        col = col.push(rule::horizontal(1));
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
                        files_col =
                            files_col.push(text("Fichier tronque (trop volumineux)").size(11));
                    }
                    // Afficher le patch dans un conteneur a largeur fixe
                    files_col = files_col.push(
                        container(scrollable(text(&diff.patch).size(12)))
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

    fn is_current(&self, repo: &str) -> bool {
        self.repo.as_deref() == Some(repo)
    }

    /// Relance la recherche avec les filtres saisis, depuis la premiere page.
    fn refresh(&mut self) -> Task<Message> {
        self.error = None;
        self.diff_error = None;
        self.selected_commit = None;
        self.diff_summaries.clear();
        self.loaded_patches.clear();
        self.expanded_files.clear();
        self.applied = self.filters.to_commit_filter();
        self.fetch(PAGE_SIZE)
    }

    /// Lit les `limit` premiers commits du filtre applique.
    fn fetch(&mut self, limit: usize) -> Task<Message> {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        self.request += 1;
        self.loading = true;
        let request = self.request;
        let filter = self.applied.clone();
        git_task(
            repo,
            move |repo| read_page(repo, &filter, limit),
            move |repo, result| Message::Loaded(repo, request, result),
        )
    }

    /// Charge les resumes de diff pour le commit selectionne.
    fn load_diff(&mut self) -> Task<Message> {
        self.diff_summaries.clear();
        self.loaded_patches.clear();
        self.expanded_files.clear();
        self.diff_error = None;

        let (Some(repo), Some(id)) = (self.repo.clone(), self.selected_commit.clone()) else {
            return Task::none();
        };
        git_task(
            repo,
            {
                let id = id.clone();
                move |repo| commit_diff_summaries(repo, &id).map_err(|err| err.to_string())
            },
            move |repo, result| Message::SummariesLoaded(repo, id.clone(), result),
        )
    }

    /// Charge le patch d'un fichier a la demande (chargement paresseux).
    fn load_file_patch(&self, file_path: String) -> Task<Message> {
        let (Some(repo), Some(id)) = (self.repo.clone(), self.selected_commit.clone()) else {
            return Task::none();
        };
        let job_id = id.clone();
        let job_path = file_path.clone();
        git_task(
            repo,
            move |repo| match commit_diff_file(repo, &job_id, &job_path) {
                Ok(Some(diff)) => Ok(diff),
                // Fichier absent du diff, inserer un espace reserve
                Ok(None) => Ok(placeholder_diff("(pas de modifications)")),
                Err(err) => Err(err.to_string()),
            },
            move |repo, result| {
                let diff = result.unwrap_or_else(|err| {
                    placeholder_diff(&format!("Erreur de chargement du diff : {err}"))
                });
                Message::PatchLoaded(repo, id.clone(), file_path.clone(), diff)
            },
        )
    }
}

/// Diff sans contenu reel, affiche a la place d'un patch absent ou en erreur.
fn placeholder_diff(patch: &str) -> FileDiff {
    FileDiff {
        additions: 0,
        deletions: 0,
        patch: patch.to_string(),
        truncated: false,
    }
}

/// Lit les `limit` premiers commits correspondant a `filter` et les branches locales.
fn read_page(repo: &str, filter: &CommitFilter, limit: usize) -> Result<LogPage, String> {
    let commits = read_commit_log(repo, filter, limit, false).map_err(|err| err.to_string())?;
    let branches = list_local_branches(repo).unwrap_or_default();
    Ok(LogPage {
        commits,
        branches,
        limit,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_page_means_more_commits_may_match() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let mut parent: Option<git2::Commit> = None;
        for i in 0..3 {
            let parents: Vec<_> = parent.iter().collect();
            let oid = repo
                .commit(Some("HEAD"), &sig, &sig, &format!("c{i}"), &tree, &parents)
                .unwrap();
            parent = Some(repo.find_commit(oid).unwrap());
        }
        let path = dir.path().to_str().unwrap();

        let page = read_page(path, &CommitFilter::default(), 2).unwrap();
        assert_eq!(page.commits.len(), page.limit);
        let page = read_page(path, &CommitFilter::default(), 4).unwrap();
        assert_eq!(page.commits.len(), 3);
        assert!(!page.branches.is_empty());
    }
}
