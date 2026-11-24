// Panneau de staging et commits — migration Iced 0.13
// Gère les fichiers staged/unstaged, les diffs, le commit editor et les stashes.
// Les appels git tournent en tâche de fond ; view() ne lit que l'état du panneau.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use git2::{Repository, Status, StatusOptions, StatusShow};
use iced::widget::{
    button, checkbox, container, horizontal_rule, pick_list, scrollable, text, text_input, Button,
    Column, Row,
};
use iced::{Element, Length, Task};

use crate::ui::theme::Theme;

use crate::git::branch::restore_file_from_branch;
use crate::git::commit::commit_index;
use crate::git::diff::diff_file;
use crate::git::stash::{apply_stash, create_stash, drop_stash, list_stashes, StashEntry};
use crate::git::status::read_repo_status;
use crate::ui::context::RepoContext;

/// Runs `job` for the repository at `repo` on the blocking thread pool and passes its result,
/// with the repository path, to `done`. Panels compare that path with their current repository
/// and drop results that arrive after a switch. Shared by the stage, branch and history panels.
pub(crate) fn git_task<M, T>(
    repo: Arc<str>,
    job: impl FnOnce(&str) -> Result<T, String> + Send + 'static,
    done: impl Fn(Arc<str>, Result<T, String>) -> M + Send + 'static,
) -> Task<M>
where
    M: Send + 'static,
    T: Send + 'static,
{
    Task::perform(
        async move {
            let path = Arc::clone(&repo);
            let result = tokio::task::spawn_blocking(move || job(&path))
                .await
                .unwrap_or_else(|err| Err(err.to_string()));
            (repo, result)
        },
        move |(repo, result)| done(repo, result),
    )
}

// ── Structures de données ──────────────────────────────────────────────────

/// Représente un fichier dans l'index ou l'arbre de travail.
#[derive(Debug, Clone)]
pub struct FileEntry {
    path: String,
    status_label: String,
}

/// État du dépôt lu en tâche de fond.
#[derive(Debug, Clone)]
pub struct Snapshot {
    staged: Vec<FileEntry>,
    unstaged: Vec<FileEntry>,
    stashes: Vec<StashEntry>,
    branch: Option<String>,
    signoff_line: Option<String>,
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
    /// Le message de commit a changé.
    CommitMessageChanged(String),
    /// Un template de commit a été sélectionné (index).
    TemplateSelected(usize),
    /// Activer/désactiver la ligne Signed-off-by.
    ToggleSignoff(bool),
    /// Committer l'index.
    Commit,
    /// Le message du stash a changé.
    StashMessageChanged(String),
    /// Inclure ou non les fichiers non suivis dans le stash.
    ToggleIncludeUntracked(bool),
    /// Créer un nouveau stash.
    CreateStash,
    /// Appliquer un stash existant (index).
    ApplyStash(usize),
    /// Demander la suppression d'un stash (index).
    DropStash(usize),
    /// Confirmer la suppression demandée.
    ConfirmDropStash,
    /// Annuler la suppression demandée.
    CancelDropStash,
    /// Ouvrir la boîte de dialogue de restauration.
    OpenRestoreDialog(String),
    /// Fermer la boîte de dialogue de restauration.
    CloseRestoreDialog,
    /// Confirmer la restauration du fichier sélectionné.
    ConfirmRestore,
    /// Sélectionner un fichier dans le dialogue de restauration.
    SelectRestoreFile(String),
    /// Résultat de la lecture du dépôt.
    Loaded(Arc<str>, Result<Snapshot, String>),
    /// Patch du fichier sélectionné ((is_staged, chemin), patch).
    DiffLoaded(Arc<str>, (bool, String), Result<String, String>),
    /// Fin d'une action git (message de statut ou erreur).
    ActionDone(Arc<str>, Result<String, String>),
    /// Fin d'un commit.
    Committed(Arc<str>, Result<String, String>),
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
    repo: Option<Arc<str>>,
    staged: Vec<FileEntry>,
    unstaged: Vec<FileEntry>,
    selected_diff: Option<(bool, String)>,
    /// Patch du fichier sélectionné, `None` pendant le chargement.
    diff: Option<String>,
    branch: Option<String>,
    status: Option<String>,
    error: Option<String>,
    /// Une action git est en cours : les boutons d'action sont désactivés.
    busy: bool,
    commit_message: String,
    include_signoff: bool,
    selected_template: usize,
    /// Ligne Signed-off-by tirée de la configuration Git, si l'identité est définie.
    signoff_line: Option<String>,
    stash_message: String,
    stashes: Vec<StashEntry>,
    include_untracked_in_stash: bool,
    /// Index du stash dont la suppression attend une confirmation.
    confirm_drop: Option<usize>,
    restore_dialog_open: bool,
    restore_selection: Option<String>,
}

impl StagePanel {
    /// Crée un nouveau panneau de staging.
    pub fn new() -> Self {
        Self {
            repo: None,
            staged: Vec::new(),
            unstaged: Vec::new(),
            selected_diff: None,
            diff: None,
            branch: None,
            status: None,
            error: None,
            busy: false,
            commit_message: String::new(),
            include_signoff: false,
            selected_template: 0,
            signoff_line: None,
            stash_message: String::from("WIP changes"),
            stashes: Vec::new(),
            include_untracked_in_stash: true,
            confirm_drop: None,
            restore_dialog_open: false,
            restore_selection: None,
        }
    }

    /// Oublie le dépôt précédent et charge `repo` en tâche de fond.
    pub fn load(&mut self, repo: &RepoContext) -> Task<Message> {
        *self = Self {
            repo: Some(repo.path_arc()),
            ..Self::new()
        };
        self.refresh()
    }

    /// Traite un message et retourne une commande Iced.
    pub fn update(&mut self, message: Message, _repo: Option<&RepoContext>) -> Task<Message> {
        match message {
            // ── Actions sur les fichiers ────────────────────────────────
            Message::ToggleStaged(path) => {
                return self.run(move |repo| {
                    unstage_path(repo, &path)
                        .map(|()| format!("Unstaged {path}"))
                        .map_err(|err| format!("Failed to unstage {path}: {err}"))
                });
            }
            Message::ToggleUnstaged(path) => {
                return self.run(move |repo| {
                    stage_path(repo, &path)
                        .map(|()| format!("Staged {path}"))
                        .map_err(|err| format!("Failed to stage {path}: {err}"))
                });
            }
            Message::ViewDiff(is_staged, path) => {
                self.selected_diff = Some((is_staged, path));
                self.diff = None;
                return self.load_diff();
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
            Message::Commit => {
                let message = self.commit_message.clone();
                return self.run_with(
                    move |repo| {
                        commit_index(repo, &message)
                            .map(|oid| format!("Commit {:.8} créé", oid.to_string()))
                            .map_err(|err| format!("Échec du commit : {err}"))
                    },
                    Message::Committed,
                );
            }

            // ── Gestion du stash ────────────────────────────────────────
            Message::StashMessageChanged(msg) => {
                self.stash_message = msg;
            }
            Message::ToggleIncludeUntracked(enabled) => {
                self.include_untracked_in_stash = enabled;
            }
            Message::CreateStash => {
                let message = self.stash_message.trim().to_string();
                let include_untracked = self.include_untracked_in_stash;
                return self.run(move |repo| {
                    create_stash(repo, &message, include_untracked)
                        .map(|()| "Stashed working tree".to_string())
                        .map_err(|err| format!("Échec du stash : {err}"))
                });
            }
            Message::ApplyStash(index) => {
                let id = self.stash_id(index);
                return self.run(move |repo| {
                    apply_stash(repo, index, &id)
                        .map(|()| format!("Applied stash #{index}"))
                        .map_err(|err| format!("Échec de l'application du stash #{index} : {err}"))
                });
            }
            Message::DropStash(index) => {
                self.confirm_drop = Some(index);
            }
            Message::CancelDropStash => {
                self.confirm_drop = None;
            }
            Message::ConfirmDropStash => {
                if let Some(index) = self.confirm_drop.take() {
                    let id = self.stash_id(index);
                    return self.run(move |repo| {
                        drop_stash(repo, index, &id)
                            .map(|()| format!("Dropped stash #{index}"))
                            .map_err(|err| {
                                format!("Échec de la suppression du stash #{index} : {err}")
                            })
                    });
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
                if let Some(path) = self.restore_selection.clone() {
                    let branch = self.restore_source().to_string();
                    return self.run(move |repo| {
                        restore_file_from_branch(repo, &branch, &path)
                            .map(|()| format!("Restored {path} from {branch}"))
                            .map_err(|err| format!("Échec de la restauration de {path} : {err}"))
                    });
                }
            }
            Message::SelectRestoreFile(path) => {
                self.restore_selection = Some(path);
            }

            // ── Résultats des tâches de fond ────────────────────────────
            Message::Loaded(repo, result) => {
                if !self.is_current(&repo) {
                    return Task::none();
                }
                match result {
                    Ok(snapshot) => return self.apply_snapshot(snapshot),
                    Err(err) => self.error = Some(err),
                }
            }
            Message::DiffLoaded(repo, key, result) => {
                if self.is_current(&repo) && self.selected_diff.as_ref() == Some(&key) {
                    self.diff =
                        Some(result.unwrap_or_else(|err| format!("Échec du chargement : {err}")));
                }
            }
            Message::ActionDone(repo, result) => {
                if self.is_current(&repo) {
                    return self.finish_action(result);
                }
            }
            Message::Committed(repo, result) => {
                if self.is_current(&repo) {
                    if result.is_ok() {
                        self.commit_message.clear();
                        self.apply_signoff();
                    }
                    return self.finish_action(result);
                }
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
        let mut content = Column::new().spacing(8).padding(10).width(Length::Fill);

        // Titre principal
        content = content.push(text("Staging & commits").size(20));
        content = content.push(
            text("Review unstaged and staged changes, preview diffs, and manage commits.").size(13),
        );

        // Vérifier si un dépôt est sélectionné
        if repo.is_none() {
            content =
                content.push(text("Open a repository to inspect and stage its changes.").size(13));
            return container(content)
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        // Erreur éventuelle, au-dessus des contrôles ; effacée à la prochaine action
        if let Some(error) = &self.error {
            content = content.push(text(error.as_str()).size(13).style(text::danger));
        }

        if self.busy {
            content = content.push(text("Opération en cours...").size(13));
        } else if let Some(status) = &self.status {
            content = content.push(text(status.as_str()).size(13));
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
            content = content.push(self.build_restore_dialog());
        }

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    // ── Construction des sous-vues ─────────────────────────────────────

    /// Bouton d'action, désactivé pendant une action git.
    fn action_button(&self, label: &str, message: Message) -> Button<'_, Message> {
        button(text(label.to_string()).size(12))
            .padding(4)
            .on_press_maybe((!self.busy).then_some(message))
    }

    /// Construit la liste de fichiers staged ou unstaged.
    fn build_file_list(&self, staged: bool) -> Element<'_, Message> {
        let title = if staged {
            "Staged files"
        } else {
            "Unstaged files"
        };
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
                self.action_button("Unstage", Message::ToggleStaged(path.clone()))
            } else {
                self.action_button("Stage", Message::ToggleUnstaged(path.clone()))
            };

            // Bouton pour voir le diff
            let diff_btn = button(text("Diff").size(12))
                .on_press(Message::ViewDiff(staged, path.clone()))
                .padding(4);

            // Bouton pour restaurer
            let restore_btn = button(text("Restore").size(12))
                .on_press(Message::OpenRestoreDialog(path))
                .padding(4);

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

        // Contenu du diff, chargé à la sélection
        if self.selected_diff.is_some() {
            let diff_text = self.diff.as_deref().unwrap_or("Chargement...");
            col = col.push(scrollable(text(diff_text).size(12)).height(Length::Fixed(180.0)));
        } else {
            col = col.push(text("Select a file to view its patch.").size(12));
        }

        container(col).width(Length::Fill).padding(8).into()
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

        // Case à cocher pour le signoff, inactive sans identité Git configurée
        col = col.push(
            checkbox("Add Signed-off-by", self.include_signoff).on_toggle_maybe(
                self.signoff_line
                    .is_some()
                    .then_some(Message::ToggleSignoff),
            ),
        );

        let can_commit = !self.commit_message.trim().is_empty() && !self.staged.is_empty();
        let mut commit_btn = self.action_button("Committer", Message::Commit);
        if !can_commit {
            commit_btn = commit_btn.on_press_maybe(None);
        }
        col = col.push(commit_btn);

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

        let untracked_checkbox = checkbox("Include untracked", self.include_untracked_in_stash)
            .on_toggle(Message::ToggleIncludeUntracked);

        let create_btn = self.action_button("Create stash", Message::CreateStash);

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

                let stash_row = Row::new()
                    .spacing(6)
                    .push(text(label).size(13).width(Length::Fill))
                    .push(self.action_button("Apply", Message::ApplyStash(idx)))
                    .push(self.action_button("Drop", Message::DropStash(idx)));
                col = col.push(stash_row);

                if self.confirm_drop == Some(idx) {
                    col = col.push(
                        Row::new()
                            .spacing(6)
                            .push(
                                text("Supprimer définitivement ce stash ?")
                                    .size(12)
                                    .width(Length::Fill),
                            )
                            .push(self.action_button("Confirmer", Message::ConfirmDropStash))
                            .push(
                                button(text("Annuler").size(12))
                                    .on_press(Message::CancelDropStash)
                                    .padding(4),
                            ),
                    );
                }
            }
        }

        scrollable(col).height(Length::Shrink).into()
    }

    /// Construit le dialogue de restauration (en ligne, pas une fenêtre modale).
    fn build_restore_dialog(&self) -> Element<'_, Message> {
        let candidates = self.restore_candidates();

        let mut col = Column::new().spacing(6).padding(8);

        col = col.push(text("Restore file").size(16));
        col = col.push(
            text(format!(
                "Restore file from {} (discard local changes).",
                self.restore_source()
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
            col = col.push(scrollable(list_col).height(Length::Fixed(180.0)));
        }

        // Boutons d'action
        let cancel_btn = button(text("Cancel").size(12))
            .on_press(Message::CloseRestoreDialog)
            .padding(4);

        // Le bouton Restore n'est actif que si un fichier est sélectionné
        let mut restore_btn = self.action_button("Restore", Message::ConfirmRestore);
        if self.restore_selection.is_none() {
            restore_btn = restore_btn.on_press_maybe(None);
        }

        let actions_row = Row::new().spacing(8).push(cancel_btn).push(restore_btn);
        col = col.push(actions_row);

        container(col).width(Length::Fill).padding(8).into()
    }

    // ── Méthodes internes ──────────────────────────────────────────────

    fn is_current(&self, repo: &str) -> bool {
        self.repo.as_deref() == Some(repo)
    }

    /// Relit les statuts, les stashes et l'identité Git en tâche de fond.
    fn refresh(&self) -> Task<Message> {
        match self.repo.clone() {
            Some(repo) => git_task(repo, read_snapshot, Message::Loaded),
            None => Task::none(),
        }
    }

    /// Charge le patch du fichier sélectionné.
    fn load_diff(&self) -> Task<Message> {
        let (Some(repo), Some(key)) = (self.repo.clone(), self.selected_diff.clone()) else {
            return Task::none();
        };
        let (staged, path) = key.clone();
        git_task(
            repo,
            move |repo| {
                diff_file(repo, &path, staged)
                    .map(|diff| {
                        diff.map_or_else(
                            || "(no textual diff available)\n".to_string(),
                            |d| d.patch,
                        )
                    })
                    .map_err(|err| err.to_string())
            },
            move |repo, result| Message::DiffLoaded(repo, key.clone(), result),
        )
    }

    /// Remplace les listes par `snapshot` et recharge le diff affiché s'il existe encore.
    fn apply_snapshot(&mut self, snapshot: Snapshot) -> Task<Message> {
        self.staged = snapshot.staged;
        self.unstaged = snapshot.unstaged;
        self.stashes = snapshot.stashes;
        self.branch = snapshot.branch;
        if snapshot.signoff_line.is_none() {
            self.include_signoff = false;
        }
        self.signoff_line = snapshot.signoff_line;

        let still_listed = self.selected_diff.as_ref().is_some_and(|(staged, path)| {
            let list = if *staged {
                &self.staged
            } else {
                &self.unstaged
            };
            list.iter().any(|entry| &entry.path == path)
        });
        if still_listed {
            self.load_diff()
        } else {
            self.selected_diff = None;
            self.diff = None;
            Task::none()
        }
    }

    /// Lance une action git si aucune n'est en cours, après avoir effacé l'erreur et le
    /// statut précédents ; la liste est relue à la fin.
    fn run(
        &mut self,
        job: impl FnOnce(&str) -> Result<String, String> + Send + 'static,
    ) -> Task<Message> {
        self.run_with(job, Message::ActionDone)
    }

    fn run_with(
        &mut self,
        job: impl FnOnce(&str) -> Result<String, String> + Send + 'static,
        done: fn(Arc<str>, Result<String, String>) -> Message,
    ) -> Task<Message> {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        if self.busy {
            return Task::none();
        }
        self.busy = true;
        self.error = None;
        self.status = None;
        git_task(repo, job, done)
    }

    /// Affiche le résultat d'une action puis relit le dépôt, y compris après une erreur
    /// (par exemple une liste de stashes qui a changé).
    fn finish_action(&mut self, result: Result<String, String>) -> Task<Message> {
        self.busy = false;
        match result {
            Ok(status) => {
                self.status = Some(status);
                self.restore_dialog_open = false;
                self.restore_selection = None;
            }
            Err(err) => self.error = Some(err),
        }
        self.refresh()
    }

    /// Identifiant du stash à `index` dans la liste affichée ; vide s'il a disparu, ce que
    /// la couche git refuse.
    fn stash_id(&self, index: usize) -> String {
        self.stashes
            .iter()
            .find(|stash| stash.index == index)
            .map(|stash| stash.id.clone())
            .unwrap_or_default()
    }

    /// Branche d'où restaurer les fichiers.
    fn restore_source(&self) -> &str {
        self.branch.as_deref().unwrap_or("HEAD")
    }

    /// Applique ou retire la ligne Signed-off-by du message.
    fn apply_signoff(&mut self) {
        let Some(line) = self.signoff_line.clone() else {
            return;
        };
        if self.include_signoff && !self.commit_message.contains(&line) {
            if !self.commit_message.ends_with('\n') && !self.commit_message.is_empty() {
                self.commit_message.push('\n');
            }
            if !self.commit_message.ends_with('\n') {
                self.commit_message.push('\n');
            }
            self.commit_message.push_str(&line);
        } else if !self.include_signoff {
            if let Some(idx) = self.commit_message.find(&line) {
                self.commit_message.replace_range(idx..idx + line.len(), "");
                self.commit_message = self.commit_message.trim_end().to_string();
            }
        }
    }

    /// Décore un message de commit avec la ligne signoff si activée.
    fn decorate_commit_message(&self, message: String) -> String {
        match &self.signoff_line {
            Some(line) if self.include_signoff => {
                let mut msg = message;
                if !msg.ends_with('\n') {
                    msg.push('\n');
                }
                msg.push_str(line);
                msg
            }
            _ => message,
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

// ── Fonctions backend ──────────────────────────────────────────────────────

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

/// Lit les fichiers modifiés, les stashes, la branche et l'identité Git. Les patches ne sont
/// pas calculés ici : seul celui du fichier sélectionné est chargé.
fn read_snapshot(repo_path: &str) -> Result<Snapshot, String> {
    let repo = Repository::open(repo_path).map_err(|err| err.to_string())?;
    let (staged, unstaged) =
        read_statuses(&repo).map_err(|err| format!("Failed to read changes: {err}"))?;
    let stashes =
        list_stashes(repo_path).map_err(|err| format!("Failed to read stashes: {err}"))?;
    let branch = read_repo_status(repo_path).ok().and_then(|s| s.branch);
    let signoff_line = repo.signature().ok().map(|sig| {
        format!(
            "Signed-off-by: {} <{}>",
            sig.name().unwrap_or_default(),
            sig.email().unwrap_or_default()
        )
    });
    Ok(Snapshot {
        staged,
        unstaged,
        stashes,
        branch,
        signoff_line,
    })
}

/// Lit les statuts staged et unstaged depuis le dépôt.
fn read_statuses(repo: &Repository) -> Result<(Vec<FileEntry>, Vec<FileEntry>), git2::Error> {
    let mut status_opts = StatusOptions::new();
    status_opts
        .show(StatusShow::IndexAndWorkdir)
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_unmodified(false);

    let statuses = repo.statuses(Some(&mut status_opts))?;

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
                path,
            });
        }
    }

    Ok((staged, unstaged))
}

/// Indexe un fichier du dépôt : son contenu s'il existe dans l'arbre de travail, sinon sa
/// suppression. `path` est relatif à la racine du dépôt, pas au dossier courant du processus.
fn stage_path(repo_path: &str, path: &str) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("bare repository has no working tree"))?;
    let mut index = repo.index()?;
    let path_ref = Path::new(path);
    // symlink_metadata: a dangling symlink is still a file to add.
    if workdir.join(path_ref).symlink_metadata().is_ok() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_path_resolves_files_in_the_repository_not_the_process_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test User").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();
        let repo_path = dir.path().to_str().unwrap();

        // Cargo runs tests from the crate root, which has a Cargo.toml and no notes file.
        // The old code checked these names there and got both of them wrong.
        let modified = "only-in-the-test-repo.txt";
        let deleted = "Cargo.toml";
        let workdir = dir.path();
        std::fs::write(workdir.join(modified), "one").unwrap();
        std::fs::write(workdir.join(deleted), "[package]").unwrap();
        stage_path(repo_path, modified).unwrap();
        stage_path(repo_path, deleted).unwrap();
        commit_index(repo_path, "initial").unwrap();

        std::fs::write(workdir.join(modified), "two").unwrap();
        std::fs::remove_file(workdir.join(deleted)).unwrap();
        stage_path(repo_path, modified).unwrap();
        stage_path(repo_path, deleted).unwrap();

        let status = |name| repo.status_file(Path::new(name)).unwrap();
        assert_eq!(status(modified), Status::INDEX_MODIFIED);
        assert_eq!(status(deleted), Status::INDEX_DELETED);
    }

    #[test]
    fn snapshot_lists_changes_without_patches_and_reads_the_signoff() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test User").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();
        std::fs::write(dir.path().join("new.txt"), "x").unwrap();

        let snapshot = read_snapshot(dir.path().to_str().unwrap()).unwrap();
        assert!(snapshot.staged.is_empty());
        assert_eq!(snapshot.unstaged[0].path, "new.txt");
        assert_eq!(
            snapshot.signoff_line.as_deref(),
            Some("Signed-off-by: Test User <test@example.com>")
        );
    }
}
