// Panneau de gestion des branches pour l'interface Iced.
// Affiche les branches locales et distantes dans une arborescence,
// permet la creation, suppression, renommage, archivage et comparaison.
// Les appels git tournent en tache de fond ; view() ne lit que l'etat du panneau.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::Utc;
use git2::BranchType;
use iced::widget::{
    button, checkbox, column, container, row, rule, scrollable, text, text_input, Button, Space,
};
use iced::{Element, Length, Task};

use crate::ui::theme::Theme;

use crate::git::branch::{
    archive_branch, checkout_branch, create_branch, create_tracking_branch, delete_branch,
    list_branches, rename_branch, BranchEntry, BranchKind,
};
use crate::git::compare::{compare_branch_with_head, BranchComparison, DiffSummary};
use crate::git::log::{commits_between_refs, latest_commit_for_branch, CommitInfo};
use crate::git::merge::{detect_conflicts, merge_branch, MergeOutcome, MergeStrategy};
use crate::git::repo_cache::with_cached_repo;
use crate::ui::context::git_task;
use crate::ui::context::RepoContext;

/// Nombre de jours sans commit avant qu'une branche soit consideree obsolete.
const STALE_DAYS: i64 = 30;

/// Nombre de branches distantes par page.
const REMOTE_PAGE_SIZE: usize = 25;

// ---------------------------------------------------------------------------
// Arborescence des branches
// ---------------------------------------------------------------------------

/// Noeud d'arborescence utilise pour regrouper les branches par prefixe (ex. feature/).
#[derive(Default)]
struct BranchNode {
    label: String,
    children: BTreeMap<String, BranchNode>,
    branch: Option<BranchEntry>,
}

impl BranchNode {
    /// Insere une branche dans l'arbre en decoupant le nom par segments.
    fn insert(&mut self, segments: &[&str], branch: BranchEntry) {
        if segments.is_empty() {
            self.branch = Some(branch);
            return;
        }

        let Some((first, rest)) = segments.split_first() else {
            return;
        };
        let node = self.children.entry(first.to_string()).or_default();
        node.label = first.to_string();
        node.insert(rest, branch);
    }
}

/// Entree aplatie de l'arborescence, prete a etre affichee dans une liste.
struct FlatEntry {
    /// Niveau d'indentation (0 = racine).
    depth: usize,
    /// Vrai si c'est un dossier (noeud intermediaire).
    is_folder: bool,
    /// Libelle affiche (segment du nom).
    label: String,
    /// Branche associee si c'est une feuille.
    branch: Option<BranchEntry>,
}

/// Aplatit recursivement un noeud en une liste de `FlatEntry`.
fn flatten_node(node: &BranchNode, depth: usize, out: &mut Vec<FlatEntry>) {
    // Si le noeud a des enfants, c'est un dossier
    if !node.children.is_empty() {
        if depth > 0 || !node.label.is_empty() {
            out.push(FlatEntry {
                depth,
                is_folder: true,
                label: node.label.clone(),
                branch: None,
            });
        }
        for child in node.children.values() {
            flatten_node(child, depth + 1, out);
        }
    } else if let Some(branch) = &node.branch {
        // Feuille : branche concrete
        out.push(FlatEntry {
            depth,
            is_folder: false,
            label: node.label.clone(),
            branch: Some(branch.clone()),
        });
    }
}

// ---------------------------------------------------------------------------
// Donnees chargees en tache de fond
// ---------------------------------------------------------------------------

/// Branches, dernier commit de chacune et conflits en cours.
#[derive(Debug, Clone)]
pub struct BranchSnapshot {
    branches: Vec<BranchEntry>,
    commits: BTreeMap<String, CommitInfo>,
    conflicts: Vec<String>,
}

/// Comparaison de la branche selectionnee avec HEAD.
#[derive(Debug, Clone)]
pub struct Selection {
    comparison: BranchComparison,
    /// `Some(false)` si la branche locale a des commits absents de HEAD ; `None` si
    /// inconnu ou branche distante.
    merged: Option<bool>,
}

/// Resultat de "Comparer avec la branche courante".
#[derive(Debug, Clone)]
pub struct Comparison {
    diff: Option<DiffSummary>,
    commits: Vec<CommitInfo>,
}

/// Action destructive en attente de confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    Delete(String),
    Merge(String),
    Rebase(String),
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Ensemble des actions possibles depuis le panneau de branches.
#[derive(Debug, Clone)]
pub enum Message {
    /// Le contenu du champ de creation a change.
    NewBranchChanged(String),
    /// Demande de creation de branche.
    CreateBranch,
    /// Selection d'une branche (par nom).
    SelectBranch(String),
    /// Basculer sur une branche.
    Checkout(String),
    /// Basculer et configurer le suivi distant.
    CheckoutAndTrack(String),
    /// Demander la suppression d'une branche locale.
    DeleteBranch(String),
    /// Archiver une branche sous forme de tag.
    ArchiveBranch(String),
    /// Renommer une branche (ancien nom, nouveau nom).
    RenameBranch(String, String),
    /// Le tampon de renommage a change.
    RenameBufferChanged(String),
    /// Demander la fusion de la branche selectionnee dans la branche courante.
    MergeInto(String),
    /// Demander le rebase de la branche courante sur la branche selectionnee
    /// (`git rebase <selectionnee>`).
    RebaseCurrentOnto(String),
    /// Executer l'action en attente de confirmation.
    Confirm,
    /// Abandonner l'action en attente de confirmation.
    CancelConfirm,
    /// Comparer la branche avec HEAD.
    CompareWithCurrent(String),
    /// Ouvrir la branche dans le panneau d'historique.
    OpenInHistory(String),
    /// Epingler / desepingler une branche.
    TogglePin(String),
    /// Basculer le filtre "branches obsoletes uniquement".
    ToggleStaleOnly,
    /// Page precedente des branches distantes.
    RemotePagePrev,
    /// Page suivante des branches distantes.
    RemotePageNext,
    /// Resultat du chargement des branches.
    Loaded(Arc<str>, Result<BranchSnapshot, String>),
    /// Details de la branche selectionnee (nom, resultat).
    SelectionLoaded(Arc<str>, String, Result<Selection, String>),
    /// Comparaison avec la branche courante (nom, resultat).
    CompareLoaded(Arc<str>, String, Result<Comparison, String>),
    /// Fin d'une action git (message de statut ou erreur).
    ActionDone(Arc<str>, Result<String, String>),
    /// Fin d'une fusion ou d'un rebase.
    MergeDone(Arc<str>, Result<MergeOutcome, String>),
}

// ---------------------------------------------------------------------------
// Panneau principal
// ---------------------------------------------------------------------------

/// Panneau de gestion des branches.
pub struct BranchPanel {
    repo: Option<Arc<str>>,
    branches: Vec<BranchEntry>,
    branch_commits: BTreeMap<String, CommitInfo>,
    new_branch: String,
    rename_buffer: String,
    selected_branch: Option<BranchEntry>,
    /// Details de la selection, `None` pendant le chargement ou apres une erreur.
    selection: Option<Selection>,
    selection_loading: bool,
    selected_error: Option<String>,
    compare_branch: Option<String>,
    compare_commits: Vec<CommitInfo>,
    compare_diff: Option<DiffSummary>,
    compare_error: Option<String>,
    error: Option<String>,
    status: Option<String>,
    conflict_files: Vec<String>,
    stale_only: bool,
    open_history_branch: Option<String>,
    pinned_branches: Vec<String>,
    pending_pinned: Option<Vec<String>>,
    remote_page: usize,
    /// Une action git est en cours : les boutons d'action sont desactives.
    busy: bool,
    pending: Option<Pending>,
}

impl BranchPanel {
    /// Cree un nouveau panneau avec les branches epinglees fournies.
    pub fn new(pinned_branches: Vec<String>) -> Self {
        Self {
            repo: None,
            branches: Vec::new(),
            branch_commits: BTreeMap::new(),
            new_branch: String::new(),
            rename_buffer: String::new(),
            selected_branch: None,
            selection: None,
            selection_loading: false,
            selected_error: None,
            compare_branch: None,
            compare_commits: Vec::new(),
            compare_diff: None,
            compare_error: None,
            error: None,
            status: None,
            conflict_files: Vec::new(),
            stale_only: false,
            open_history_branch: None,
            pinned_branches,
            pending_pinned: None,
            remote_page: 0,
            busy: false,
            pending: None,
        }
    }

    /// Oublie le depot precedent et charge les branches de `repo` en tache de fond.
    /// Les branches epinglees sont conservees.
    pub fn load(&mut self, repo: &RepoContext) -> Task<Message> {
        let pinned = std::mem::take(&mut self.pinned_branches);
        *self = Self {
            repo: Some(repo.path_arc()),
            ..Self::new(pinned)
        };
        self.refresh()
    }

    /// Met a jour les branches epinglees depuis l'exterieur.
    pub fn set_pinned_branches(&mut self, pinned_branches: Vec<String>) {
        self.pinned_branches = pinned_branches;
    }

    /// Recupere et consomme les changements d'epingles en attente.
    pub fn take_pinned_changes(&mut self) -> Option<Vec<String>> {
        self.pending_pinned.take()
    }

    /// Recupere et consomme la demande d'ouverture dans l'historique.
    pub fn take_history_request(&mut self) -> Option<String> {
        self.open_history_branch.take()
    }

    // -----------------------------------------------------------------------
    // Mise a jour (logique)
    // -----------------------------------------------------------------------

    /// Traite un message. Les operations git tournent en tache de fond.
    pub fn update(&mut self, message: Message, _repo: Option<&RepoContext>) -> Task<Message> {
        match message {
            Message::NewBranchChanged(value) => {
                self.new_branch = value;
            }

            Message::CreateBranch => {
                let name = self.new_branch.trim().to_string();
                if name.is_empty() {
                    self.status = None;
                    self.error = Some("Le nom de branche ne peut pas etre vide".to_string());
                } else {
                    // The name is repeated in the error message if creation fails.
                    self.new_branch.clear();
                    return self.run(move |repo| {
                        create_branch(repo, &name, None)
                            .map(|()| format!("Branche {name} creee depuis HEAD"))
                            .map_err(|err| format!("Echec de la creation de branche : {err}"))
                    });
                }
            }

            Message::SelectBranch(name) => {
                if let Some(branch) = self.branches.iter().find(|b| b.name == name).cloned() {
                    self.rename_buffer = branch.name.clone();
                    self.selected_branch = Some(branch);
                    self.pending = None;
                    return self.load_selection();
                }
            }

            Message::Checkout(name) => {
                return self.run(move |repo| {
                    checkout_branch(repo, &name)
                        .map(|()| format!("Basculement sur {name}"))
                        .map_err(|err| format!("Echec du checkout de {name} : {err}"))
                });
            }

            Message::CheckoutAndTrack(name) => {
                return self.run(move |repo| {
                    let local = create_tracking_branch(repo, &name).map_err(|e| e.to_string())?;
                    checkout_branch(repo, &local)
                        .map(|()| format!("Branche {local} creee et suivie depuis {name}"))
                        .map_err(|err| format!("Echec du checkout de {local} : {err}"))
                });
            }

            Message::DeleteBranch(name) => {
                self.pending = Some(Pending::Delete(name));
            }

            Message::ArchiveBranch(name) => {
                return self.run(move |repo| {
                    archive_branch(repo, &name)
                        .map(|tag| format!("Branche {name} archivee sous le tag {tag}"))
                        .map_err(|err| err.to_string())
                });
            }

            Message::RenameBranch(old, new) => {
                if !new.is_empty() && new != old {
                    return self.run(move |repo| {
                        rename_branch(repo, &old, &new)
                            .map(|()| format!("Branche {old} renommee en {new}"))
                            .map_err(|err| err.to_string())
                    });
                }
            }

            Message::RenameBufferChanged(value) => {
                self.rename_buffer = value;
            }

            Message::MergeInto(name) => {
                self.pending = Some(Pending::Merge(name));
            }

            Message::RebaseCurrentOnto(name) => {
                self.pending = Some(Pending::Rebase(name));
            }

            Message::CancelConfirm => {
                self.pending = None;
            }

            Message::Confirm => match self.pending.take() {
                Some(Pending::Delete(name)) => {
                    return self.run(move |repo| {
                        delete_branch(repo, &name)
                            .map(|()| format!("Branche {name} supprimee"))
                            .map_err(|err| err.to_string())
                    });
                }
                Some(Pending::Merge(name)) => {
                    return self.run_merge(name, MergeStrategy::Merge);
                }
                Some(Pending::Rebase(name)) => {
                    return self.run_merge(name, MergeStrategy::Rebase);
                }
                None => {}
            },

            Message::CompareWithCurrent(name) => {
                self.compare_branch = Some(name.clone());
                self.compare_error = None;
                self.compare_diff = None;
                self.compare_commits.clear();
                if let Some(repo) = self.repo.clone() {
                    return git_task(
                        repo,
                        {
                            let name = name.clone();
                            move |repo| load_comparison(repo, &name)
                        },
                        move |repo, result| Message::CompareLoaded(repo, name.clone(), result),
                    );
                }
            }

            Message::OpenInHistory(name) => {
                self.open_history_branch = Some(name);
            }

            Message::TogglePin(name) => {
                self.toggle_pin_by_name(&name);
            }

            Message::ToggleStaleOnly => {
                self.stale_only = !self.stale_only;
            }

            Message::RemotePagePrev => {
                self.remote_page = self.remote_page.saturating_sub(1);
            }

            Message::RemotePageNext => {
                // La borne superieure est verifiee lors du rendu
                self.remote_page = self.remote_page.saturating_add(1);
            }

            // -- Resultats des taches de fond --
            Message::Loaded(repo, result) => {
                if self.is_current(&repo) {
                    match result {
                        Ok(snapshot) => return self.apply_snapshot(snapshot),
                        Err(err) => self.error = Some(err),
                    }
                }
            }

            Message::SelectionLoaded(repo, name, result) => {
                let selected = self.selected_branch.as_ref().map(|b| b.name.as_str());
                if self.is_current(&repo) && selected == Some(name.as_str()) {
                    self.selection_loading = false;
                    match result {
                        Ok(selection) => self.selection = Some(selection),
                        Err(err) => {
                            self.selected_error =
                                Some(format!("Echec de la comparaison de branche : {err}"));
                        }
                    }
                }
            }

            Message::CompareLoaded(repo, name, result) => {
                if self.is_current(&repo) && self.compare_branch.as_deref() == Some(name.as_str()) {
                    match result {
                        Ok(comparison) => {
                            self.compare_diff = comparison.diff;
                            self.compare_commits = comparison.commits;
                        }
                        Err(err) => self.compare_error = Some(err),
                    }
                }
            }

            Message::ActionDone(repo, result) => {
                if self.is_current(&repo) {
                    self.busy = false;
                    match result {
                        Ok(status) => self.status = Some(status),
                        Err(err) => self.error = Some(err),
                    }
                    return self.refresh();
                }
            }

            Message::MergeDone(repo, result) => {
                if self.is_current(&repo) {
                    self.busy = false;
                    match result {
                        Ok(outcome) => self.handle_merge_outcome(outcome),
                        Err(err) => self.error = Some(err),
                    }
                    return self.refresh();
                }
            }
        }

        Task::none()
    }

    // -----------------------------------------------------------------------
    // Vue (rendu)
    // -----------------------------------------------------------------------

    /// Construit l'arbre de widgets Iced pour le panneau de branches.
    pub fn view<'a>(
        &'a self,
        _theme: &'a Theme,
        repo: Option<&'a RepoContext>,
    ) -> Element<'a, Message> {
        let mut main_col = column![].spacing(8).padding(8);

        // Titre et description
        main_col = main_col
            .push(text("Branch explorer").size(24))
            .push(text("Navigate branches, manage them, and perform git operations.").size(14));

        // Sans depot ouvert, message d'aide
        if repo.is_none() {
            main_col = main_col
                .push(text("Open a repository to browse and manage its branches.").size(14));
            return main_col.into();
        }

        // Erreur globale, au-dessus des controles ; effacee a la prochaine action
        if let Some(error) = &self.error {
            main_col = main_col.push(text(error.as_str()).size(14).style(text::danger));
        }

        // Conflits de fusion
        if !self.conflict_files.is_empty() {
            let msg = format!(
                "Conflits de fusion detectes dans : {}",
                self.conflict_files.join(", ")
            );
            main_col = main_col.push(text(msg).size(14));
        }

        // Message de statut
        if self.busy {
            main_col = main_col.push(text("Operation en cours...").size(14));
        } else if let Some(status) = &self.status {
            main_col = main_col.push(text(status.as_str()).size(14));
        }

        // Barre de creation
        main_col = main_col.push(self.view_creation_bar());

        // Case a cocher "branches obsoletes uniquement"
        main_col = main_col.push(
            checkbox(self.stale_only)
                .label("Show stale only")
                .on_toggle(|_| Message::ToggleStaleOnly),
        );

        // Disposition en deux colonnes : locales | distantes
        let local_col = self.view_branch_tree(BranchKind::Local, "Local branches");
        let remote_col = self.view_branch_tree(BranchKind::Remote, "Remote branches");

        let two_columns = row![
            container(local_col).width(Length::FillPortion(1)),
            container(remote_col).width(Length::FillPortion(1)),
        ]
        .spacing(16);

        main_col = main_col.push(two_columns);

        // Panneau de details de la selection (sous la colonne gauche)
        main_col = main_col
            .push(rule::horizontal(1))
            .push(self.view_selection_panel());

        // Panneau de comparaison
        main_col = main_col
            .push(rule::horizontal(1))
            .push(self.view_compare_panel());

        main_col.into()
    }

    /// Bouton d'action, desactive pendant une action git.
    fn action_button<'a>(&self, label: &'a str, message: Message) -> Button<'a, Message> {
        button(text(label)).on_press_maybe((!self.busy).then_some(message))
    }

    /// Barre de creation de branche (champ texte + bouton).
    fn view_creation_bar(&self) -> Element<'_, Message> {
        let input = text_input("feature/my-branch", &self.new_branch)
            .on_input(Message::NewBranchChanged)
            .on_submit(Message::CreateBranch);

        let create_btn = self.action_button("Create", Message::CreateBranch);

        container(row![text("New branch").size(14), input, create_btn].spacing(8))
            .padding(8)
            .into()
    }

    /// Construit la vue d'une colonne de branches (locales ou distantes).
    fn view_branch_tree<'a>(&'a self, kind: BranchKind, heading: &'a str) -> Element<'a, Message> {
        let mut col = column![].spacing(4);
        col = col.push(text(heading).size(20));

        // Filtrer et partitionner les branches visibles
        let visible: Vec<&BranchEntry> = self
            .branches
            .iter()
            .filter(|b| b.kind == kind && self.should_show_branch(b))
            .collect();

        let (pinned, unpinned): (Vec<_>, Vec<_>) =
            visible.into_iter().partition(|b| self.is_branch_pinned(b));

        // Pagination pour les branches distantes
        let (page_branches, total_pages) = if kind == BranchKind::Remote {
            let total = unpinned.len().div_ceil(REMOTE_PAGE_SIZE).max(1);
            let page = self.remote_page.min(total.saturating_sub(1));
            let start = page * REMOTE_PAGE_SIZE;
            let end = (start + REMOTE_PAGE_SIZE).min(unpinned.len());
            (&unpinned[start..end], total)
        } else {
            (&unpinned[..], 1usize)
        };

        // Construire l'arbre a partir des branches de la page
        let mut root = BranchNode::default();
        for branch in page_branches {
            let segments: Vec<&str> = branch.name.split('/').collect();
            root.insert(&segments, (*branch).clone());
        }

        // Branches epinglees
        if !pinned.is_empty() {
            col = col.push(text("Pinned").size(14));
            for branch in &pinned {
                col = col.push(self.view_branch_button(branch));
            }
            if !root.children.is_empty() {
                col = col.push(rule::horizontal(1));
            }
        }

        if pinned.is_empty() && root.children.is_empty() {
            col = col.push(text("No branches found.").size(14));
            return scrollable(col).into();
        }

        // Aplatir l'arbre et afficher
        let mut flat: Vec<FlatEntry> = Vec::new();
        for child in root.children.values() {
            flatten_node(child, 0, &mut flat);
        }

        for entry in flat {
            let indent = entry.depth as u16 * 16;
            if entry.is_folder {
                col = col.push(
                    row![
                        Space::new().width(Length::Fixed(indent as f32)),
                        text(entry.label).size(14),
                    ]
                    .spacing(4),
                );
            } else if let Some(branch) = &entry.branch {
                col = col.push(
                    row![
                        Space::new().width(Length::Fixed(indent as f32)),
                        self.view_branch_button(branch),
                    ]
                    .spacing(4),
                );
            }
        }

        // Pagination distante
        if kind == BranchKind::Remote && total_pages > 1 {
            let current_page = self.remote_page.min(total_pages.saturating_sub(1));
            let prev_btn = if current_page > 0 {
                button(text("<")).on_press(Message::RemotePagePrev)
            } else {
                button(text("<"))
            };
            let next_btn = if current_page + 1 < total_pages {
                button(text(">")).on_press(Message::RemotePageNext)
            } else {
                button(text(">"))
            };
            let page_label = text(format!("Page {}/{}", current_page + 1, total_pages)).size(14);
            col = col.push(row![prev_btn, page_label, next_btn].spacing(8));
        }

        scrollable(col).into()
    }

    /// Bouton pour une branche individuelle dans la liste.
    fn view_branch_button<'a>(&self, branch: &BranchEntry) -> Element<'a, Message> {
        let mut label = branch.name.clone();
        if branch.is_head {
            label.push_str(" (HEAD)");
        }

        let is_stale = self.is_branch_stale(branch);
        let branch_name = branch.name.clone();

        let mut entry_row = row![].spacing(4);
        entry_row = entry_row.push(
            button(text(label).size(14))
                .on_press(Message::SelectBranch(branch_name))
                .padding(4),
        );

        if is_stale {
            entry_row = entry_row.push(container(text("stale").size(11)).padding([2, 6]));
        }

        entry_row.into()
    }

    /// Panneau affichant les details de la branche selectionnee.
    fn view_selection_panel(&self) -> Element<'_, Message> {
        let mut col = column![].spacing(6);
        col = col.push(text("Selection details").size(20));

        let Some(branch) = &self.selected_branch else {
            col = col
                .push(text("Select a branch to see its details and available actions.").size(14));
            return col.into();
        };

        // Erreur de selection
        if let Some(error) = &self.selected_error {
            col = col.push(text(error.as_str()).size(14));
        }

        // Nom et type de branche
        col = col.push(text(&branch.name).size(16));
        let kind_label = match branch.kind {
            BranchKind::Local => "Local branch",
            BranchKind::Remote => "Remote branch",
        };
        col = col.push(text(kind_label).size(12));

        // Informations de comparaison
        if self.selection_loading {
            col = col.push(text("Chargement...").size(12));
        }
        if let Some(comparison) = self.selection.as_ref().map(|s| &s.comparison) {
            if let Some(commit) = &comparison.commit {
                col = col
                    .push(text(&commit.summary).size(14))
                    .push(text(format!("by {}", commit.author)).size(12));
            }
            if let Some(diff) = &comparison.diff {
                col = col.push(
                    text(format!(
                        "{} files  +{} / -{}",
                        diff.files_changed, diff.additions, diff.deletions
                    ))
                    .size(12),
                );
            }
        }

        // Action en attente de confirmation
        if let Some(pending) = &self.pending {
            col = col.push(self.view_confirmation(pending));
        }

        // Actions
        col = col.push(text("Actions").size(14));

        let branch_name = branch.name.clone();
        let is_pinned = self.is_branch_pinned(branch);
        let pin_label = if is_pinned { "Unpin" } else { "Pin" };

        col = col.push(button(text(pin_label)).on_press(Message::TogglePin(branch_name.clone())));

        // Checkout
        let mut checkout_row = row![].spacing(8);
        checkout_row = checkout_row
            .push(self.action_button("Checkout", Message::Checkout(branch_name.clone())));
        if branch.kind == BranchKind::Remote {
            checkout_row = checkout_row.push(self.action_button(
                "Checkout & Track",
                Message::CheckoutAndTrack(branch_name.clone()),
            ));
        }
        col = col.push(checkout_row);

        // Merge et Rebase
        col = col.push(
            row![
                self.action_button(
                    "Merge into current",
                    Message::MergeInto(branch_name.clone())
                ),
                self.action_button(
                    "Rebaser la branche courante sur celle-ci",
                    Message::RebaseCurrentOnto(branch_name.clone())
                ),
            ]
            .spacing(8),
        );

        // Comparer et Historique
        col = col.push(
            row![
                button(text("Compare with current"))
                    .on_press(Message::CompareWithCurrent(branch_name.clone())),
                button(text("Open in History"))
                    .on_press(Message::OpenInHistory(branch_name.clone())),
            ]
            .spacing(8),
        );

        // Actions specifiques aux branches locales
        if branch.kind == BranchKind::Local {
            let mut local_row = row![].spacing(8);

            if !branch.is_head {
                // Attendre la verification de fusion avant de proposer la suppression
                let mut delete_btn =
                    self.action_button("Delete", Message::DeleteBranch(branch_name.clone()));
                if self.selection_loading {
                    delete_btn = delete_btn.on_press_maybe(None);
                }
                local_row = local_row.push(delete_btn);
            }

            local_row = local_row
                .push(self.action_button("Archive", Message::ArchiveBranch(branch_name.clone())));

            col = col.push(local_row);

            // Renommage
            let old_name = branch_name.clone();
            let new_name = self.rename_buffer.trim().to_string();
            let rename_btn = if !new_name.is_empty() && new_name != old_name {
                self.action_button("Apply", Message::RenameBranch(old_name, new_name))
            } else {
                button(text("Apply"))
            };

            col = col.push(
                row![
                    text("Rename:").size(14),
                    text_input("new name", &self.rename_buffer)
                        .on_input(Message::RenameBufferChanged),
                    rename_btn,
                ]
                .spacing(8),
            );
        }

        col.into()
    }

    /// Demande de confirmation d'une action destructive.
    fn view_confirmation(&self, pending: &Pending) -> Element<'_, Message> {
        let current = self.current_branch().unwrap_or("HEAD");
        let mut lines = column![].spacing(4);
        match pending {
            Pending::Delete(name) => {
                lines = lines.push(text(format!("Supprimer la branche {name} ?")).size(14));
                if self.selection.as_ref().and_then(|s| s.merged) == Some(false) {
                    lines = lines.push(
                        text(format!(
                            "{name} n'est pas fusionnee dans {current} : ses commits \
                             propres ne seront plus accessibles par une branche."
                        ))
                        .size(14)
                        .style(text::danger),
                    );
                }
            }
            Pending::Merge(name) => {
                lines = lines.push(text(format!("Fusionner {name} dans {current} ?")).size(14));
            }
            Pending::Rebase(name) => {
                lines = lines.push(
                    text(format!(
                        "Rebaser {current} sur {name} ? Les commits de {current} seront reecrits."
                    ))
                    .size(14),
                );
            }
        }
        lines = lines.push(
            row![
                self.action_button("Confirmer", Message::Confirm),
                button(text("Annuler")).on_press(Message::CancelConfirm),
            ]
            .spacing(8),
        );
        container(lines).padding(8).into()
    }

    /// Panneau de comparaison avec la branche courante.
    fn view_compare_panel(&self) -> Element<'_, Message> {
        let mut col = column![].spacing(6);
        col = col.push(text("Compare with current").size(20));

        let Some(branch_name) = &self.compare_branch else {
            col = col.push(
                text("Select 'Compare with current' on a branch to see differences.").size(14),
            );
            return col.into();
        };

        if let Some(error) = &self.compare_error {
            col = col.push(text(error.as_str()).size(14));
            return col.into();
        }

        col = col.push(text(branch_name.as_str()).size(16));

        if let Some(diff) = &self.compare_diff {
            col = col.push(
                text(format!(
                    "{} files changed  +{} / -{}",
                    diff.files_changed, diff.additions, diff.deletions
                ))
                .size(14),
            );
        }

        col = col.push(text("Commits between current HEAD and branch").size(16));

        if self.compare_commits.is_empty() {
            col = col.push(text("No commits found in the selected range.").size(14));
            return col.into();
        }

        let mut commits_col = column![].spacing(4);
        for commit in &self.compare_commits {
            let short_id = short_id(&commit.id);
            commits_col = commits_col.push(
                column![
                    text(&commit.summary).size(14),
                    text(format!("{}  {}", short_id, commit.author)).size(12),
                ]
                .spacing(2),
            );
        }

        col = col.push(scrollable(commits_col).height(Length::Fixed(220.0)));

        col.into()
    }

    // -----------------------------------------------------------------------
    // Logique interne
    // -----------------------------------------------------------------------

    fn is_current(&self, repo: &str) -> bool {
        self.repo.as_deref() == Some(repo)
    }

    /// Relit les branches, leur dernier commit et les conflits en tache de fond.
    fn refresh(&self) -> Task<Message> {
        match self.repo.clone() {
            Some(repo) => git_task(repo, read_snapshot, Message::Loaded),
            None => Task::none(),
        }
    }

    /// Remplace la liste ; la selection est rechargee si la branche existe encore.
    fn apply_snapshot(&mut self, snapshot: BranchSnapshot) -> Task<Message> {
        self.branches = snapshot.branches;
        self.branch_commits = snapshot.commits;
        self.conflict_files = snapshot.conflicts;

        let still_listed = self.selected_branch.as_ref().and_then(|selected| {
            self.branches
                .iter()
                .find(|b| b.name == selected.name && b.kind == selected.kind)
                .cloned()
        });
        match still_listed {
            Some(branch) => {
                self.selected_branch = Some(branch);
                self.load_selection()
            }
            None => {
                self.selected_branch = None;
                self.selection = None;
                self.pending = None;
                Task::none()
            }
        }
    }

    /// Charge la comparaison de la branche selectionnee avec HEAD.
    fn load_selection(&mut self) -> Task<Message> {
        let (Some(repo), Some(branch)) = (self.repo.clone(), self.selected_branch.clone()) else {
            return Task::none();
        };
        self.selection = None;
        self.selected_error = None;
        self.selection_loading = true;
        let name = branch.name.clone();
        git_task(
            repo,
            move |repo| {
                let comparison =
                    compare_branch_with_head(repo, &branch.name).map_err(|e| e.to_string())?;
                let merged = (branch.kind == BranchKind::Local)
                    .then(|| merged_into_head(repo, &branch.name).ok())
                    .flatten();
                Ok(Selection { comparison, merged })
            },
            move |repo, result| Message::SelectionLoaded(repo, name.clone(), result),
        )
    }

    /// Lance une action git si aucune n'est en cours, apres avoir efface l'erreur et le
    /// statut precedents ; la liste est relue a la fin.
    fn run(
        &mut self,
        job: impl FnOnce(&str) -> Result<String, String> + Send + 'static,
    ) -> Task<Message> {
        match self.start() {
            Some(repo) => git_task(repo, job, Message::ActionDone),
            None => Task::none(),
        }
    }

    /// Lance une fusion ou un rebase de la branche courante avec `branch`.
    fn run_merge(&mut self, branch: String, strategy: MergeStrategy) -> Task<Message> {
        match self.start() {
            Some(repo) => git_task(
                repo,
                move |repo| merge_branch(repo, &branch, strategy),
                Message::MergeDone,
            ),
            None => Task::none(),
        }
    }

    /// Marque le debut d'une action ; `None` si une action tourne deja ou sans depot.
    fn start(&mut self) -> Option<Arc<str>> {
        if self.busy {
            return None;
        }
        let repo = self.repo.clone()?;
        self.busy = true;
        self.status = None;
        self.error = None;
        Some(repo)
    }

    /// Gere le resultat d'une operation de fusion.
    fn handle_merge_outcome(&mut self, outcome: MergeOutcome) {
        if outcome.had_conflicts {
            self.conflict_files = outcome.conflicts;
            self.status = Some(
                "Conflits detectes. Resolvez-les dans votre repertoire de travail.".to_string(),
            );
        } else {
            self.conflict_files.clear();
            self.status = Some(outcome.message);
        }
    }

    /// Nom de la branche locale courante.
    fn current_branch(&self) -> Option<&str> {
        self.branches
            .iter()
            .find(|b| b.is_head && b.kind == BranchKind::Local)
            .map(|b| b.name.as_str())
    }

    /// Epingle ou desepingle une branche par nom.
    fn toggle_pin_by_name(&mut self, name: &str) {
        if let Some(pos) = self.pinned_branches.iter().position(|n| n == name) {
            self.pinned_branches.remove(pos);
        } else {
            self.pinned_branches.push(name.to_string());
        }
        self.pending_pinned = Some(self.pinned_branches.clone());
    }

    /// Verifie si une branche est consideree obsolete (pas de commit recent).
    fn is_branch_stale(&self, branch: &BranchEntry) -> bool {
        let Some(commit) = self.branch_commits.get(&branch_key(branch)) else {
            return true;
        };
        let age_seconds = Utc::now().timestamp().saturating_sub(commit.time.seconds());
        age_seconds > STALE_DAYS * 24 * 60 * 60
    }

    /// Determine si une branche doit etre affichee selon les filtres actifs.
    fn should_show_branch(&self, branch: &BranchEntry) -> bool {
        !self.stale_only || self.is_branch_stale(branch)
    }

    /// Verifie si une branche est epinglee.
    fn is_branch_pinned(&self, branch: &BranchEntry) -> bool {
        self.pinned_branches.iter().any(|name| name == &branch.name)
    }
}

// ---------------------------------------------------------------------------
// Lectures git (tache de fond)
// ---------------------------------------------------------------------------

/// Cle unique pour indexer les commits par branche.
fn branch_key(branch: &BranchEntry) -> String {
    match branch.kind {
        BranchKind::Local => format!("local:{}", branch.name),
        BranchKind::Remote => format!("remote:{}", branch.name),
    }
}

/// Lit les branches, le dernier commit de chacune et les conflits en cours.
fn read_snapshot(repo: &str) -> Result<BranchSnapshot, String> {
    let branches =
        list_branches(repo).map_err(|err| format!("Echec de lecture des branches : {err}"))?;
    let mut commits = BTreeMap::new();
    for branch in &branches {
        let commit = latest_commit_for_branch(repo, &branch.name)
            .map_err(|err| format!("Echec de lecture de l'historique de branche : {err}"))?;
        if let Some(commit) = commit {
            commits.insert(branch_key(branch), commit);
        }
    }
    let conflicts =
        detect_conflicts(repo).map_err(|err| format!("Echec de detection des conflits : {err}"))?;
    Ok(BranchSnapshot {
        branches,
        commits,
        conflicts,
    })
}

/// Charge l'ecart et les commits entre HEAD et `branch`.
fn load_comparison(repo: &str, branch: &str) -> Result<Comparison, String> {
    let diff = compare_branch_with_head(repo, branch)
        .map_err(|err| format!("Echec de la comparaison de branche : {err}"))?
        .diff;
    let commits = commits_between_refs(repo, "HEAD", branch, 50)
        .map_err(|err| format!("Echec du chargement des commits de comparaison : {err}"))?;
    Ok(Comparison { diff, commits })
}

/// Vrai si chaque commit de la branche locale `name` est accessible depuis HEAD.
fn merged_into_head(repo_path: &str, name: &str) -> Result<bool, git2::Error> {
    with_cached_repo(repo_path, |repo| {
        let tip = repo
            .find_branch(name, BranchType::Local)?
            .get()
            .peel_to_commit()?
            .id();
        let head = repo.head()?.peel_to_commit()?.id();
        Ok(tip == head || repo.graph_descendant_of(head, tip)?)
    })
}

/// Raccourcit un identifiant de commit a 7 caracteres.
fn short_id(id: &str) -> String {
    id.chars().take(7).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_check_sees_commits_missing_from_head() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let base = repo
            .commit(Some("HEAD"), &sig, &sig, "base", &tree, &[])
            .unwrap();
        let base = repo.find_commit(base).unwrap();
        repo.branch("merged", &base, false).unwrap();
        // A commit on `ahead` only, with HEAD left on the base commit.
        let ahead = repo
            .commit(None, &sig, &sig, "ahead", &tree, &[&base])
            .unwrap();
        repo.branch("ahead", &repo.find_commit(ahead).unwrap(), false)
            .unwrap();

        let path = dir.path().to_str().unwrap();
        assert!(merged_into_head(path, "merged").unwrap());
        assert!(!merged_into_head(path, "ahead").unwrap());
    }
}
