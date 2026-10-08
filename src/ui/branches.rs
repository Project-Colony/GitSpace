// Panneau de gestion des branches pour l'interface Iced.
// Affiche les branches locales et distantes dans une arborescence,
// permet la creation, suppression, renommage, archivage et comparaison.

use std::collections::BTreeMap;

use chrono::Utc;
use iced::widget::{
    button, checkbox, column, container, horizontal_rule, row, scrollable, text, text_input, Space,
};
use iced::{Element, Length};

use crate::ui::theme::Theme;

use crate::git::branch::{
    BranchEntry, BranchKind, archive_branch, checkout_branch, create_branch,
    create_tracking_branch, delete_branch, list_branches, rename_branch,
};
use crate::git::compare::{BranchComparison, DiffSummary, compare_branch_with_head};
use crate::git::log::{CommitInfo, commits_between_refs, latest_commit_for_branch};
use crate::git::merge::{MergeOutcome, MergeStrategy, detect_conflicts, merge_branch};
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
    /// Supprimer une branche locale.
    DeleteBranch(String),
    /// Archiver une branche sous forme de tag.
    ArchiveBranch(String),
    /// Renommer une branche (ancien nom, nouveau nom).
    RenameBranch(String, String),
    /// Le tampon de renommage a change.
    RenameBufferChanged(String),
    /// Fusionner la branche selectionnee dans la branche courante.
    MergeInto(String),
    /// Rebaser la branche selectionnee sur la branche courante.
    RebaseOnto(String),
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
}

// ---------------------------------------------------------------------------
// Panneau principal
// ---------------------------------------------------------------------------

/// Panneau de gestion des branches.
pub struct BranchPanel {
    branches: Vec<BranchEntry>,
    branch_commits: BTreeMap<String, CommitInfo>,
    new_branch: String,
    rename_buffer: String,
    last_repo: Option<String>,
    selected_branch: Option<BranchEntry>,
    selected_comparison: Option<BranchComparison>,
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
}

impl BranchPanel {
    /// Cree un nouveau panneau avec les branches epinglees fournies.
    pub fn new(pinned_branches: Vec<String>) -> Self {
        Self {
            branches: Vec::new(),
            branch_commits: BTreeMap::new(),
            new_branch: String::new(),
            rename_buffer: String::new(),
            last_repo: None,
            selected_branch: None,
            selected_comparison: None,
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
        }
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

    /// Traite un message et effectue l'operation git correspondante.
    /// Toutes les operations sont synchrones (suffisamment rapides).
    pub fn update(
        &mut self,
        message: Message,
        repo: Option<&RepoContext>,
    ) -> iced::Task<Message> {
        match message {
            Message::NewBranchChanged(value) => {
                self.new_branch = value;
            }

            Message::CreateBranch => {
                if let Some(repo) = repo {
                    self.status = None;
                    self.error = None;
                    let name = self.new_branch.trim().to_string();
                    if name.is_empty() {
                        self.error = Some("Le nom de branche ne peut pas etre vide".to_string());
                    } else {
                        match create_branch(repo.path(), &name, None) {
                            Ok(()) => {
                                self.status =
                                    Some(format!("Branche {name} creee depuis HEAD"));
                                self.new_branch.clear();
                                self.refresh(repo);
                            }
                            Err(err) => {
                                self.error =
                                    Some(format!("Echec de la creation de branche : {err}"));
                            }
                        }
                    }
                }
            }

            Message::SelectBranch(name) => {
                if let Some(repo) = repo {
                    if let Some(branch) = self.branches.iter().find(|b| b.name == name).cloned() {
                        self.select_branch(repo, &branch);
                    }
                }
            }

            Message::Checkout(name) => {
                if let Some(repo) = repo {
                    self.run_branch_action(repo, || checkout_branch(repo.path(), &name));
                }
            }

            Message::CheckoutAndTrack(name) => {
                if let Some(repo) = repo {
                    self.run_branch_action(repo, || {
                        let local_name = create_tracking_branch(repo.path(), &name)?;
                        checkout_branch(repo.path(), &local_name)?;
                        Ok(())
                    });
                }
            }

            Message::DeleteBranch(name) => {
                if let Some(repo) = repo {
                    self.run_branch_action(repo, || delete_branch(repo.path(), &name));
                    self.selected_branch = None;
                }
            }

            Message::ArchiveBranch(name) => {
                if let Some(repo) = repo {
                    self.status = None;
                    self.error = None;
                    match archive_branch(repo.path(), &name) {
                        Ok(tag) => {
                            self.status =
                                Some(format!("Branche {name} archivee sous le tag {tag}"));
                            self.refresh(repo);
                            self.selected_branch = None;
                        }
                        Err(err) => self.error = Some(err.to_string()),
                    }
                }
            }

            Message::RenameBranch(old, new) => {
                if let Some(repo) = repo {
                    if !new.is_empty() && new != old {
                        self.run_branch_action(repo, || rename_branch(repo.path(), &old, &new));
                        self.selected_branch = None;
                    }
                }
            }

            Message::RenameBufferChanged(value) => {
                self.rename_buffer = value;
            }

            Message::MergeInto(name) => {
                if let Some(repo) = repo {
                    self.run_merge_action(repo, &name, MergeStrategy::Merge);
                }
            }

            Message::RebaseOnto(name) => {
                if let Some(repo) = repo {
                    self.run_merge_action(repo, &name, MergeStrategy::Rebase);
                }
            }

            Message::CompareWithCurrent(name) => {
                if let Some(repo) = repo {
                    self.compare_with_current(repo, &name);
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
        }

        iced::Task::none()
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
            .push(text(
                "Navigate branches, manage them, and perform git operations.",
            ).size(14));

        // Sans depot ouvert, message d'aide
        let Some(repo) = repo else {
            main_col = main_col.push(
                text("Open a repository to browse and manage its branches.").size(14),
            );
            return main_col.into();
        };

        // Erreur globale
        if let Some(error) = &self.error {
            main_col = main_col.push(text(error.as_str()).size(14));
            return main_col.into();
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
        if let Some(status) = &self.status {
            main_col = main_col.push(text(status.as_str()).size(14));
        }

        // Barre de creation
        main_col = main_col.push(self.view_creation_bar());

        // Case a cocher "branches obsoletes uniquement"
        main_col = main_col.push(
            checkbox("Show stale only", self.stale_only)
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
            .push(horizontal_rule(1))
            .push(self.view_selection_panel(repo));

        // Panneau de comparaison
        main_col = main_col
            .push(horizontal_rule(1))
            .push(self.view_compare_panel());

        main_col.into()
    }

    /// Barre de creation de branche (champ texte + bouton).
    fn view_creation_bar(&self) -> Element<'_, Message> {
        let input = text_input("feature/my-branch", &self.new_branch)
            .on_input(Message::NewBranchChanged)
            .on_submit(Message::CreateBranch);

        let create_btn = button(text("Create")).on_press(Message::CreateBranch);

        container(
            row![text("New branch").size(14), input, create_btn].spacing(8),
        )
        .padding(8)
        .into()
    }

    /// Construit la vue d'une colonne de branches (locales ou distantes).
    fn view_branch_tree<'a>(
        &'a self,
        kind: BranchKind,
        heading: &'a str,
    ) -> Element<'a, Message> {
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
                col = col.push(horizontal_rule(1));
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
                        Space::with_width(Length::Fixed(indent as f32)),
                        text(entry.label).size(14),
                    ]
                    .spacing(4),
                );
            } else if let Some(branch) = &entry.branch {
                col = col.push(
                    row![
                        Space::with_width(Length::Fixed(indent as f32)),
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
            entry_row = entry_row.push(
                container(text("stale").size(11)).padding([2, 6]),
            );
        }

        entry_row.into()
    }

    /// Panneau affichant les details de la branche selectionnee.
    fn view_selection_panel<'a>(
        &'a self,
        _repo: &'a RepoContext,
    ) -> Element<'a, Message> {
        let mut col = column![].spacing(6);
        col = col.push(text("Selection details").size(20));

        let Some(branch) = &self.selected_branch else {
            col = col.push(
                text("Select a branch to see its details and available actions.").size(14),
            );
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
        if let Some(comparison) = &self.selected_comparison {
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

        // Actions
        col = col.push(text("Actions").size(14));

        let branch_name = branch.name.clone();
        let is_pinned = self.is_branch_pinned(branch);
        let pin_label = if is_pinned { "Unpin" } else { "Pin" };

        col = col.push(
            button(text(pin_label)).on_press(Message::TogglePin(branch_name.clone())),
        );

        // Checkout
        let mut checkout_row = row![].spacing(8);
        checkout_row = checkout_row.push(
            button(text("Checkout")).on_press(Message::Checkout(branch_name.clone())),
        );
        if branch.kind == BranchKind::Remote {
            checkout_row = checkout_row.push(
                button(text("Checkout & Track"))
                    .on_press(Message::CheckoutAndTrack(branch_name.clone())),
            );
        }
        col = col.push(checkout_row);

        // Merge et Rebase
        col = col.push(
            row![
                button(text("Merge into current"))
                    .on_press(Message::MergeInto(branch_name.clone())),
                button(text("Rebase onto current"))
                    .on_press(Message::RebaseOnto(branch_name.clone())),
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
                local_row = local_row.push(
                    button(text("Delete"))
                        .on_press(Message::DeleteBranch(branch_name.clone())),
                );
            }

            local_row = local_row.push(
                button(text("Archive"))
                    .on_press(Message::ArchiveBranch(branch_name.clone())),
            );

            col = col.push(local_row);

            // Renommage
            let old_name = branch_name.clone();
            let new_name = self.rename_buffer.trim().to_string();
            let rename_btn = if !new_name.is_empty() && new_name != old_name {
                button(text("Apply")).on_press(Message::RenameBranch(old_name, new_name))
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

    /// Rafraichit la liste des branches et les commits associes.
    pub fn refresh(&mut self, repo: &RepoContext) {
        if self.last_repo.as_deref() != Some(repo.path()) {
            self.branches.clear();
            self.status = None;
            self.error = None;
            self.last_repo = Some(repo.path().to_string());
            self.selected_branch = None;
            self.selected_comparison = None;
            self.selected_error = None;
            self.compare_branch = None;
            self.compare_commits.clear();
            self.compare_diff = None;
            self.compare_error = None;
            self.remote_page = 0;
        }

        match list_branches(repo.path()) {
            Ok(branches) => self.branches = branches,
            Err(err) => {
                self.error = Some(format!("Echec de lecture des branches : {err}"));
                return;
            }
        }

        self.refresh_branch_commits(repo);

        match detect_conflicts(repo.path()) {
            Ok(conflicts) => self.conflict_files = conflicts,
            Err(err) => {
                self.error = Some(format!("Echec de detection des conflits : {err}"));
            }
        }
    }

    /// Charge le dernier commit de chaque branche.
    fn refresh_branch_commits(&mut self, repo: &RepoContext) {
        self.branch_commits.clear();
        for branch in &self.branches {
            match latest_commit_for_branch(repo.path(), &branch.name) {
                Ok(Some(commit)) => {
                    self.branch_commits.insert(self.branch_key(branch), commit);
                }
                Ok(None) => {}
                Err(err) => {
                    self.error =
                        Some(format!("Echec de lecture de l'historique de branche : {err}"));
                    return;
                }
            }
        }
    }

    /// Execute une action git generique, puis rafraichit.
    fn run_branch_action<F>(&mut self, repo: &RepoContext, action: F)
    where
        F: FnOnce() -> Result<(), git2::Error>,
    {
        self.status = None;
        self.error = None;
        match action() {
            Ok(()) => {
                self.status = Some("Operation terminee".to_string());
                self.refresh(repo);
            }
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    /// Execute une fusion ou un rebase.
    fn run_merge_action(&mut self, repo: &RepoContext, branch: &str, strategy: MergeStrategy) {
        self.status = None;
        self.error = None;
        match merge_branch(repo.path(), branch, strategy) {
            Ok(outcome) => self.handle_merge_outcome(repo, outcome),
            Err(err) => self.error = Some(err),
        }
    }

    /// Gere le resultat d'une operation de fusion.
    fn handle_merge_outcome(&mut self, repo: &RepoContext, outcome: MergeOutcome) {
        if outcome.had_conflicts {
            self.conflict_files = outcome.conflicts;
            self.status = Some(
                "Conflits detectes. Resolvez-les dans votre repertoire de travail.".to_string(),
            );
        } else {
            self.conflict_files.clear();
            self.status = Some(outcome.message);
        }
        self.refresh(repo);
    }

    /// Selectionne une branche et charge sa comparaison avec HEAD.
    fn select_branch(&mut self, repo: &RepoContext, branch: &BranchEntry) {
        self.selected_branch = Some(branch.clone());
        self.selected_error = None;
        self.rename_buffer = branch.name.clone();
        match compare_branch_with_head(repo.path(), &branch.name) {
            Ok(comparison) => self.selected_comparison = Some(comparison),
            Err(err) => {
                self.selected_comparison = None;
                self.selected_error =
                    Some(format!("Echec de la comparaison de branche : {err}"));
            }
        }
    }

    /// Charge les donnees de comparaison entre une branche et HEAD.
    fn compare_with_current(&mut self, repo: &RepoContext, branch_name: &str) {
        self.compare_branch = Some(branch_name.to_string());
        self.compare_error = None;
        match compare_branch_with_head(repo.path(), branch_name) {
            Ok(comparison) => self.compare_diff = comparison.diff,
            Err(err) => {
                self.compare_diff = None;
                self.compare_error =
                    Some(format!("Echec de la comparaison de branche : {err}"));
            }
        }

        match commits_between_refs(repo.path(), "HEAD", branch_name, 50) {
            Ok(commits) => self.compare_commits = commits,
            Err(err) => {
                self.compare_commits.clear();
                self.compare_error =
                    Some(format!("Echec du chargement des commits de comparaison : {err}"));
            }
        }
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

    /// Cle unique pour indexer les commits par branche.
    fn branch_key(&self, branch: &BranchEntry) -> String {
        match branch.kind {
            BranchKind::Local => format!("local:{}", branch.name),
            BranchKind::Remote => format!("remote:{}", branch.name),
        }
    }

    /// Verifie si une branche est consideree obsolete (pas de commit recent).
    fn is_branch_stale(&self, branch: &BranchEntry) -> bool {
        let key = self.branch_key(branch);
        let Some(commit) = self.branch_commits.get(&key) else {
            return true;
        };
        let age_seconds = Utc::now().timestamp().saturating_sub(commit.time.seconds());
        age_seconds > STALE_DAYS * 24 * 60 * 60
    }

    /// Determine si une branche doit etre affichee selon les filtres actifs.
    fn should_show_branch(&self, branch: &BranchEntry) -> bool {
        if self.stale_only && !self.is_branch_stale(branch) {
            return false;
        }
        true
    }

    /// Verifie si une branche est epinglee.
    fn is_branch_pinned(&self, branch: &BranchEntry) -> bool {
        self.pinned_branches.iter().any(|name| name == &branch.name)
    }
}

/// Raccourcit un identifiant de commit a 7 caracteres.
fn short_id(id: &str) -> String {
    id.chars().take(7).collect()
}
