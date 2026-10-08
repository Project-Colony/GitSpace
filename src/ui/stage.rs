// Panneau de staging et commits — migration Iced 0.13
// Gère les fichiers staged/unstaged, les diffs, le commit editor et les stashes.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use git2::{Repository, Signature, Status, StatusOptions, StatusShow};
use iced::widget::{
    button, checkbox, container, horizontal_rule, pick_list, scrollable, text, text_input, Column, Row,
};
use iced::{Element, Length, Task};

use crate::ui::theme::Theme;

use crate::git::branch::restore_file_from_branch;
use crate::git::diff::{diff_file, staged_diff, working_tree_diff};
use crate::git::stash::{StashEntry, apply_stash, create_stash, drop_stash, list_stashes};
use crate::git::status::read_repo_status;
use crate::ui::context::RepoContext;

// ── Structures de données ──────────────────────────────────────────────────

/// Représente un fichier dans l'index ou l'arbre de travail.
#[derive(Debug, Clone)]
struct FileEntry {
    path: String,
    status_label: String,
    diff: String,
}

/// Messages émis par le panneau de staging.
#[derive(Debug, Clone)]
pub enum Message {
    /// Désindexer un fichier (chemin).
    ToggleStaged(String),
    /// Indexer un fichier (chemin).
    ToggleUnstaged(String),
    /// Afficher le diff d'un fichier (is_staged, chemin).
    ViewDiff(bool, String),
    /// Restaurer un fichier depuis la branche courante.
    RestoreFile(String),
    /// Le message de commit a changé.
    CommitMessageChanged(String),
    /// Un template de commit a été sélectionné (index).
    TemplateSelected(usize),
    /// Activer/désactiver la ligne Signed-off-by.
    ToggleSignoff(bool),
    /// Le message du stash a changé.
    StashMessageChanged(String),
    /// Inclure ou non les fichiers non suivis dans le stash.
    ToggleIncludeUntracked(bool),
    /// Créer un nouveau stash.
    CreateStash,
    /// Appliquer un stash existant (index).
    ApplyStash(usize),
    /// Supprimer un stash existant (index).
    DropStash(usize),
    /// Ouvrir la boîte de dialogue de restauration.
    OpenRestoreDialog(String),
    /// Fermer la boîte de dialogue de restauration.
    CloseRestoreDialog,
    /// Confirmer la restauration du fichier sélectionné.
    ConfirmRestore,
    /// Sélectionner un fichier dans le dialogue de restauration.
    SelectRestoreFile(String),
}

// ── Templates de commit ────────────────────────────────────────────────────

const COMMIT_TEMPLATES: &[(&str, &str)] = &[
    ("WIP", "WIP: describe the work in progress"),
    (
        "Feature",
        "feat: short summary\n\n- describe the change\n- add context or links",
    ),
    (
        "Fix",
        "fix: bug summary\n\nExplain root cause and how it was addressed.",
    ),
];

/// Noms des templates pour le sélecteur.
fn template_names() -> Vec<String> {
    COMMIT_TEMPLATES
        .iter()
        .map(|(name, _)| name.to_string())
        .collect()
}

// ── Panneau principal ──────────────────────────────────────────────────────

pub struct StagePanel {
    staged: Vec<FileEntry>,
    unstaged: Vec<FileEntry>,
    selected_diff: Option<(bool, String)>,
    last_repo: Option<String>,
    status: Option<String>,
    error: Option<String>,
    commit_message: String,
    include_signoff: bool,
    selected_template: usize,
    signoff_line: String,
    stash_message: String,
    stashes: Vec<StashEntry>,
    include_untracked_in_stash: bool,
    needs_refresh: bool,
    restore_dialog_open: bool,
    restore_selection: Option<String>,
}

impl StagePanel {
    /// Crée un nouveau panneau de staging.
    pub fn new() -> Self {
        let signoff_line = default_signoff_line();
        Self {
            staged: Vec::new(),
            unstaged: Vec::new(),
            selected_diff: None,
            last_repo: None,
            status: None,
            error: None,
            commit_message: String::new(),
            include_signoff: false,
            selected_template: 0,
            signoff_line,
            stash_message: String::from("WIP changes"),
            stashes: Vec::new(),
            include_untracked_in_stash: true,
            needs_refresh: true,
            restore_dialog_open: false,
            restore_selection: None,
        }
    }

    /// Traite un message et retourne une commande Iced.
    pub fn update(
        &mut self,
        message: Message,
        repo: Option<&RepoContext>,
    ) -> Task<Message> {
        match message {
            // ── Actions sur les fichiers ────────────────────────────────
            Message::ToggleStaged(path) => {
                // Désindexer le fichier
                if let Some(repo) = repo {
                    self.handle_unstage(repo, &path);
                }
            }
            Message::ToggleUnstaged(path) => {
                // Indexer le fichier
                if let Some(repo) = repo {
                    self.handle_stage(repo, &path);
                }
            }
            Message::ViewDiff(is_staged, path) => {
                self.selected_diff = Some((is_staged, path));
            }
            Message::RestoreFile(path) => {
                // Raccourci : ouvrir le dialogue et sélectionner le fichier
                self.restore_dialog_open = true;
                self.restore_selection = Some(path);
            }

            // ── Éditeur de commit ───────────────────────────────────────
            Message::CommitMessageChanged(msg) => {
                self.commit_message = msg;
            }
            Message::TemplateSelected(idx) => {
                if idx < COMMIT_TEMPLATES.len() {
                    self.selected_template = idx;
                    self.commit_message =
                        self.decorate_commit_message(COMMIT_TEMPLATES[idx].1.to_string());
                }
            }
            Message::ToggleSignoff(enabled) => {
                self.include_signoff = enabled;
                self.apply_signoff();
            }

            // ── Gestion du stash ────────────────────────────────────────
            Message::StashMessageChanged(msg) => {
                self.stash_message = msg;
            }
            Message::ToggleIncludeUntracked(enabled) => {
                self.include_untracked_in_stash = enabled;
            }
            Message::CreateStash => {
                if let Some(repo) = repo {
                    self.status = None;
                    match create_stash(
                        repo.path(),
                        self.stash_message.trim(),
                        self.include_untracked_in_stash,
                    ) {
                        Ok(_) => {
                            self.status = Some("Stashed working tree".to_string());
                            self.needs_refresh = true;
                        }
                        Err(err) => {
                            self.error = Some(format!("Échec du stash : {err}"));
                        }
                    }
                }
            }
            Message::ApplyStash(index) => {
                if let Some(repo) = repo {
                    match apply_stash(repo.path(), index) {
                        Ok(_) => {
                            self.status = Some(format!("Applied stash #{index}"));
                            self.needs_refresh = true;
                        }
                        Err(err) => {
                            self.error =
                                Some(format!("Échec de l'application du stash #{index} : {err}"));
                        }
                    }
                }
            }
            Message::DropStash(index) => {
                if let Some(repo) = repo {
                    match drop_stash(repo.path(), index) {
                        Ok(_) => {
                            self.status = Some(format!("Dropped stash #{index}"));
                            self.needs_refresh = true;
                        }
                        Err(err) => {
                            self.error =
                                Some(format!("Échec de la suppression du stash #{index} : {err}"));
                        }
                    }
                }
            }

            // ── Dialogue de restauration ────────────────────────────────
            Message::OpenRestoreDialog(path) => {
                self.restore_dialog_open = true;
                self.restore_selection = Some(path);
            }
            Message::CloseRestoreDialog => {
                self.restore_dialog_open = false;
                self.restore_selection = None;
            }
            Message::ConfirmRestore => {
                if let (Some(repo), Some(path)) = (repo, self.restore_selection.clone()) {
                    let current_branch = read_repo_status(repo.path())
                        .ok()
                        .and_then(|s| s.branch)
                        .unwrap_or_else(|| "HEAD".to_string());
                    match restore_file_from_branch(repo.path(), &current_branch, &path) {
                        Ok(()) => {
                            self.status =
                                Some(format!("Restored {path} from {current_branch}"));
                            self.needs_refresh = true;
                            self.restore_dialog_open = false;
                            self.restore_selection = None;
                        }
                        Err(err) => {
                            self.error =
                                Some(format!("Échec de la restauration de {path} : {err}"));
                        }
                    }
                }
            }
            Message::SelectRestoreFile(path) => {
                self.restore_selection = Some(path);
            }
        }

        // Rafraîchir les données si nécessaire après une action
        if self.needs_refresh {
            if let Some(repo) = repo {
                self.refresh_data(repo);
            }
        }

        Task::none()
    }

    /// Construit la vue du panneau de staging.
    pub fn view<'a>(
        &'a self,
        _theme: &'a Theme,
        repo: Option<&'a RepoContext>,
    ) -> Element<'a, Message> {
        let mut content = Column::new()
            .spacing(8)
            .padding(10)
            .width(Length::Fill);

        // Titre principal
        content = content.push(text("Staging & commits").size(20));
        content = content.push(
            text("Review unstaged and staged changes, preview diffs, and manage commits.")
                .size(13),
        );

        // Vérifier si un dépôt est sélectionné
        let repo = match repo {
            Some(r) => r,
            None => {
                content = content.push(
                    text("Open a repository to inspect and stage its changes.").size(13),
                );
                return container(content)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            }
        };

        // Afficher les erreurs éventuelles
        if let Some(error) = &self.error {
            content = content.push(text(error.clone()).size(13));
            return container(content)
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        // Afficher le statut si présent
        if let Some(status) = &self.status {
            content = content.push(text(status.clone()).size(13));
        }

        // ── Deux colonnes : unstaged | staged ──────────────────────────
        let unstaged_col = self.build_file_list(false);
        let staged_col = self.build_file_list(true);

        let files_row = Row::new()
            .spacing(10)
            .width(Length::Fill)
            .push(
                container(unstaged_col)
                    .width(Length::FillPortion(1))
                    .height(Length::Fixed(260.0)),
            )
            .push(
                container(staged_col)
                    .width(Length::FillPortion(1))
                    .height(Length::Fixed(260.0)),
            );
        content = content.push(files_row);

        // ── Section de prévisualisation du diff ─────────────────────────
        content = content.push(self.build_diff_preview());

        // ── Éditeur de commit + contrôles du stash côte à côte ──────────
        let commit_stash_row = Row::new()
            .spacing(10)
            .width(Length::Fill)
            .push(
                container(self.build_commit_editor())
                    .width(Length::FillPortion(3))
                    .padding(10),
            )
            .push(
                container(self.build_stash_controls())
                    .width(Length::FillPortion(2))
                    .padding(10),
            );
        content = content.push(commit_stash_row);

        // ── Dialogue de restauration (intégré dans le panneau) ──────────
        if self.restore_dialog_open {
            content = content.push(horizontal_rule(1));
            content = content.push(self.build_restore_dialog(repo));
        }

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    // ── Construction des sous-vues ─────────────────────────────────────

    /// Construit la liste de fichiers staged ou unstaged.
    fn build_file_list(&self, staged: bool) -> Element<'_, Message> {
        let title = if staged { "Staged files" } else { "Unstaged files" };
        let list = if staged { &self.staged } else { &self.unstaged };

        let mut col = Column::new().spacing(4).push(text(title).size(16));

        if list.is_empty() {
            col = col.push(text("No files in this section.").size(12));
            return scrollable(col).height(Length::Fill).into();
        }

        for entry in list {
            let path = entry.path.clone();
            let label = format!("{} ({})", entry.path, entry.status_label);

            // Bouton pour stage/unstage
            let toggle_btn = if staged {
                let p = path.clone();
                button(text("Unstage").size(12))
                    .on_press(Message::ToggleStaged(p))
                    .padding(4)
            } else {
                let p = path.clone();
                button(text("Stage").size(12))
                    .on_press(Message::ToggleUnstaged(p))
                    .padding(4)
            };

            // Bouton pour voir le diff
            let diff_btn = {
                let p = path.clone();
                button(text("Diff").size(12))
                    .on_press(Message::ViewDiff(staged, p))
                    .padding(4)
            };

            // Bouton pour restaurer
            let restore_btn = {
                let p = path.clone();
                button(text("Restore").size(12))
                    .on_press(Message::OpenRestoreDialog(p))
                    .padding(4)
            };

            let file_row = Row::new()
                .spacing(6)
                .push(text(label).size(13).width(Length::Fill))
                .push(toggle_btn)
                .push(diff_btn)
                .push(restore_btn);

            col = col.push(file_row);
        }

        scrollable(col).height(Length::Fill).into()
    }

    /// Construit la section de prévisualisation du diff.
    fn build_diff_preview(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(4).padding(8);

        // En-tête du diff
        let header_row = match &self.selected_diff {
            Some((is_staged, path)) => {
                let kind = if *is_staged { "staged" } else { "unstaged" };
                Row::new()
                    .spacing(8)
                    .push(text("Diff preview").size(16))
                    .push(text(format!("— {path} ({kind})")).size(13))
            }
            None => Row::new().push(text("Diff preview").size(16)),
        };
        col = col.push(header_row);
        col = col.push(horizontal_rule(1));

        // Contenu du diff
        match &self.selected_diff {
            Some((is_staged, path)) => {
                let list = if *is_staged { &self.staged } else { &self.unstaged };
                let diff_text = list
                    .iter()
                    .find(|f| &f.path == path)
                    .map(|f| f.diff.as_str())
                    .unwrap_or("(file not found in current list)");
                col = col.push(
                    scrollable(text(diff_text).size(12))
                        .height(Length::Fixed(180.0)),
                );
            }
            None => {
                col = col.push(text("Select a file to view its patch.").size(12));
            }
        }

        container(col)
            .width(Length::Fill)
            .padding(8)
            .into()
    }

    /// Construit l'éditeur de commit.
    fn build_commit_editor(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(6);

        col = col.push(text("Commit editor").size(16));

        // Sélection de template via pick_list
        let names = template_names();
        let selected = names.get(self.selected_template).cloned();
        let template_picker = pick_list(names, selected, move |name| {
            // Retrouver l'index à partir du nom
            let idx = COMMIT_TEMPLATES
                .iter()
                .position(|(n, _)| *n == name)
                .unwrap_or(0);
            Message::TemplateSelected(idx)
        })
        .placeholder("Template...");

        let template_row = Row::new()
            .spacing(8)
            .push(text("Template").size(13))
            .push(template_picker);
        col = col.push(template_row);

        // Zone de texte pour le message de commit
        col = col.push(
            text_input("Write your commit message...", &self.commit_message)
                .on_input(Message::CommitMessageChanged)
                .padding(6),
        );

        // Case à cocher pour le signoff
        col = col.push(
            checkbox("Add Signed-off-by", self.include_signoff)
                .on_toggle(Message::ToggleSignoff),
        );

        col.into()
    }

    /// Construit les contrôles de gestion du stash.
    fn build_stash_controls(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(6);

        col = col.push(text("Stash management").size(16));

        // Ligne de saisie + options
        let stash_input = text_input("Describe the stash...", &self.stash_message)
            .on_input(Message::StashMessageChanged)
            .padding(4);

        let untracked_checkbox =
            checkbox("Include untracked", self.include_untracked_in_stash)
                .on_toggle(Message::ToggleIncludeUntracked);

        let create_btn = button(text("Create stash").size(12))
            .on_press(Message::CreateStash)
            .padding(4);

        let stash_row = Row::new()
            .spacing(6)
            .push(stash_input)
            .push(untracked_checkbox)
            .push(create_btn);
        col = col.push(stash_row);

        col = col.push(text("Apply or drop an existing stash.").size(12));

        // Liste des stashes existants
        if self.stashes.is_empty() {
            col = col.push(text("No stashes available.").size(12));
        } else {
            for stash in &self.stashes {
                let idx = stash.index;
                let label = format!("#{} — {}", stash.index, stash.message);

                let apply_btn = button(text("Apply").size(12))
                    .on_press(Message::ApplyStash(idx))
                    .padding(4);
                let drop_btn = button(text("Drop").size(12))
                    .on_press(Message::DropStash(idx))
                    .padding(4);

                let stash_row = Row::new()
                    .spacing(6)
                    .push(text(label).size(13).width(Length::Fill))
                    .push(apply_btn)
                    .push(drop_btn);
                col = col.push(stash_row);
            }
        }

        scrollable(col).height(Length::Shrink).into()
    }

    /// Construit le dialogue de restauration (en ligne, pas une fenêtre modale).
    fn build_restore_dialog<'a>(&'a self, repo: &'a RepoContext) -> Element<'a, Message> {
        let current_branch = read_repo_status(repo.path())
            .ok()
            .and_then(|s| s.branch)
            .unwrap_or_else(|| "HEAD".to_string());

        let candidates = self.restore_candidates();

        let mut col = Column::new().spacing(6).padding(8);

        col = col.push(text("Restore file").size(16));
        col = col.push(
            text(format!(
                "Restore file from {current_branch} (discard local changes)."
            ))
            .size(13),
        );

        if candidates.is_empty() {
            col = col.push(text("No modified files to restore.").size(12));
        } else {
            // Liste sélectionnable de fichiers
            let mut list_col = Column::new().spacing(2);
            for path in &candidates {
                let is_selected = self.restore_selection.as_deref() == Some(path.as_str());
                let label = if is_selected {
                    format!("> {path}")
                } else {
                    path.clone()
                };
                let p = path.clone();
                list_col = list_col.push(
                    button(text(label).size(12))
                        .on_press(Message::SelectRestoreFile(p))
                        .padding(4)
                        .width(Length::Fill),
                );
            }
            col = col.push(
                scrollable(list_col).height(Length::Fixed(180.0)),
            );
        }

        // Boutons d'action
        let cancel_btn = button(text("Cancel").size(12))
            .on_press(Message::CloseRestoreDialog)
            .padding(4);

        // Le bouton Restore n'est actif que si un fichier est sélectionné
        let restore_btn = if self.restore_selection.is_some() {
            button(text("Restore").size(12))
                .on_press(Message::ConfirmRestore)
                .padding(4)
        } else {
            button(text("Restore").size(12)).padding(4)
        };

        let actions_row = Row::new()
            .spacing(8)
            .push(cancel_btn)
            .push(restore_btn);
        col = col.push(actions_row);

        container(col)
            .width(Length::Fill)
            .padding(8)
            .into()
    }

    // ── Méthodes internes ──────────────────────────────────────────────

    /// Rafraîchit les statuts et les stashes depuis le dépôt.
    fn refresh_data(&mut self, repo: &RepoContext) {
        // Détecter un changement de dépôt
        if self.last_repo.as_deref() != Some(repo.path()) {
            self.last_repo = Some(repo.path().to_string());
            self.selected_diff = None;
            self.commit_message.clear();
            self.status = None;
            self.error = None;
            self.needs_refresh = true;
        }

        if !self.needs_refresh {
            return;
        }

        match read_statuses(repo.path()) {
            Ok((staged, unstaged)) => {
                self.staged = staged;
                self.unstaged = unstaged;
                self.error = None;
            }
            Err(err) => {
                self.error = Some(format!("Failed to read changes: {err}"));
            }
        }

        match list_stashes(repo.path()) {
            Ok(entries) => self.stashes = entries,
            Err(err) => {
                self.error = Some(format!("Failed to read stashes: {err}"));
            }
        }

        self.needs_refresh = false;
    }

    /// Rafraîchir les données si un dépôt est fourni.
    pub fn refresh_if_needed(&mut self, repo: &RepoContext) {
        self.refresh_data(repo);
    }

    /// Indexe un fichier dans le dépôt.
    fn handle_stage(&mut self, repo: &RepoContext, path: &str) {
        self.status = None;
        match stage_path(repo.path(), path) {
            Ok(_) => {
                self.status = Some(format!("Staged {path}"));
                self.needs_refresh = true;
            }
            Err(err) => self.error = Some(format!("Failed to stage {path}: {err}")),
        }
    }

    /// Désindexe un fichier du dépôt.
    fn handle_unstage(&mut self, repo: &RepoContext, path: &str) {
        self.status = None;
        match unstage_path(repo.path(), path) {
            Ok(_) => {
                self.status = Some(format!("Unstaged {path}"));
                self.needs_refresh = true;
            }
            Err(err) => self.error = Some(format!("Failed to unstage {path}: {err}")),
        }
    }

    /// Applique ou retire la ligne Signed-off-by du message.
    fn apply_signoff(&mut self) {
        if self.include_signoff && !self.commit_message.contains(&self.signoff_line) {
            if !self.commit_message.ends_with('\n') && !self.commit_message.is_empty() {
                self.commit_message.push('\n');
            }
            if !self.commit_message.ends_with('\n') {
                self.commit_message.push('\n');
            }
            self.commit_message.push_str(&self.signoff_line);
        } else if !self.include_signoff {
            if let Some(idx) = self.commit_message.find(&self.signoff_line) {
                self.commit_message
                    .replace_range(idx..idx + self.signoff_line.len(), "");
                self.commit_message = self.commit_message.trim_end().to_string();
            }
        }
    }

    /// Décore un message de commit avec la ligne signoff si activée.
    fn decorate_commit_message(&self, message: String) -> String {
        if self.include_signoff {
            let mut msg = message;
            if !msg.ends_with('\n') {
                msg.push('\n');
            }
            msg.push_str(&self.signoff_line);
            msg
        } else {
            message
        }
    }

    /// Retourne la liste triée des fichiers modifiés (candidats à la restauration).
    fn restore_candidates(&self) -> Vec<String> {
        let mut candidates = BTreeSet::new();
        for entry in self.staged.iter().chain(self.unstaged.iter()) {
            candidates.insert(entry.path.clone());
        }
        candidates.into_iter().collect()
    }
}

// ── Fonctions backend (inchangées) ─────────────────────────────────────────

/// Formate un label lisible à partir du statut git2.
fn format_status_label(status: Status) -> String {
    if status.is_wt_new() || status.is_index_new() {
        "added".to_string()
    } else if status.is_wt_deleted() || status.is_index_deleted() {
        "deleted".to_string()
    } else if status.is_wt_modified() || status.is_index_modified() {
        "modified".to_string()
    } else if status.is_wt_renamed() || status.is_index_renamed() {
        "renamed".to_string()
    } else {
        "changed".to_string()
    }
}

/// Lit les statuts staged et unstaged depuis le dépôt.
fn read_statuses(repo_path: &str) -> Result<(Vec<FileEntry>, Vec<FileEntry>), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let mut status_opts = StatusOptions::new();
    status_opts
        .show(StatusShow::IndexAndWorkdir)
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_unmodified(false);

    let statuses = repo.statuses(Some(&mut status_opts))?;
    let staged_map = build_diff_map(staged_diff(repo_path)?);
    let unstaged_map = build_diff_map(working_tree_diff(repo_path)?);

    let mut staged = Vec::new();
    let mut unstaged = Vec::new();

    for entry in statuses.iter() {
        let path = entry.path().unwrap_or("(unknown)").to_string();
        let status = entry.status();

        if status.is_index_new()
            || status.is_index_modified()
            || status.is_index_deleted()
            || status.is_index_renamed()
            || status.is_index_typechange()
        {
            staged.push(FileEntry {
                status_label: format_status_label(status),
                diff: lookup_or_refresh_diff(&staged_map, repo_path, &path, true)?,
                path: path.clone(),
            });
        }

        if status.is_wt_new()
            || status.is_wt_modified()
            || status.is_wt_deleted()
            || status.is_wt_renamed()
            || status.is_wt_typechange()
        {
            unstaged.push(FileEntry {
                status_label: format_status_label(status),
                diff: lookup_or_refresh_diff(&unstaged_map, repo_path, &path, false)?,
                path,
            });
        }
    }

    Ok((staged, unstaged))
}

/// Construit une table de correspondance chemin -> patch.
fn build_diff_map(diffs: Vec<crate::git::diff::FileDiff>) -> HashMap<String, String> {
    diffs
        .into_iter()
        .map(|diff| (diff.path, diff.patch))
        .collect()
}

/// Cherche un diff dans la table, ou le recalcule à la volée.
fn lookup_or_refresh_diff(
    diffs: &HashMap<String, String>,
    repo_path: &str,
    path: &str,
    staged: bool,
) -> Result<String, git2::Error> {
    if let Some(patch) = diffs.get(path) {
        return Ok(patch.clone());
    }

    let patch = diff_file(repo_path, path, staged)?
        .map(|entry| entry.patch)
        .unwrap_or_else(|| "(no textual diff available)\n".to_string());
    Ok(patch)
}

/// Indexe un fichier dans le dépôt git.
fn stage_path(repo_path: &str, path: &str) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let mut index = repo.index()?;
    let path_ref = Path::new(path);
    if path_ref.exists() {
        index.add_path(path_ref)?;
    } else {
        index.remove_path(path_ref)?;
    }
    index.write()
}

/// Désindexe un fichier du dépôt git.
fn unstage_path(repo_path: &str, path: &str) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    repo.reset_default(None, [Path::new(path)])
}

/// Retourne la ligne Signed-off-by par défaut.
fn default_signoff_line() -> String {
    match Signature::now("GitSpace", "gitspace@example.com") {
        Ok(sig) => format!(
            "Signed-off-by: {} <{}>",
            sig.name().unwrap_or("GitSpace"),
            sig.email().unwrap_or("gitspace@example.com")
        ),
        Err(_) => "Signed-off-by: GitSpace <gitspace@example.com>".to_string(),
    }
}
