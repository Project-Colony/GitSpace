//! The Branches tab: the branch list grouped as local, then one group per remote, and the
//! selected branch with its relation card, its actions and its own commits.
//! Git reads and actions run in background tasks; `view` only reads the panel's state.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use colony_ui::theme::Palette;
use colony_ui::widgets::icons;
use colony_ui::{fonts, Typography};
use git2::Oid;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input};
use iced::widget::{Column, Space};
use iced::{Alignment, Border, Element, Length, Task};

use crate::git::branch::{
    archive_branch, checkout_branch, create_branch, create_tracking_branch, delete_branch,
    head_commit, list_branch_overview, rename_branch, set_upstream, BranchKind, BranchOverview,
};
use crate::git::log::{commits_between_refs, read_commit_log, CommitFilter, CommitInfo};
use crate::git::merge::{merge_branch, MergeStrategy};
use crate::i18n::{t, tf};
use crate::ui::changes::{field, horizontal_divider, item, question, vertical_divider};
use crate::ui::context::{git_task, RepoContext};
use crate::ui::history::relative_time;
use crate::ui::widgets::button as gs_button;
use crate::ui::widgets::menu::{self, Entry, MenuItem};
use crate::ui::widgets::segmented::segmented;
use crate::ui::widgets::toast::Toast;

/// Days without a commit before a branch counts as stale.
const STALE_DAYS: i64 = 30;
/// Remote branches per page of a remote group.
const REMOTE_PAGE_SIZE: usize = 25;
/// Commits listed under "Commits only on <branch>".
const OWN_COMMITS: usize = 30;
/// Commits listed under "Latest commits on <branch>" for the current branch.
const LATEST_COMMITS: usize = 10;
const LIST_WIDTH: f32 = 280.0;
const MORE_GLYPH: &str = "\u{f141}";
const CURRENT_GLYPH: &str = "\u{f111}";
const STAR_GLYPH: &str = "\u{f005}";
const BRANCH_GLYPH: &str = "\u{e725}";
const CLOUD_GLYPH: &str = "\u{f0c2}";
const PREVIOUS_GLYPH: &str = "\u{f053}";

/// What can be done with a branch, in the order the screen shows it: the actions under the
/// relation card, then the "⋯" menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchAction {
    Checkout,
    Merge,
    Rebase,
    Rename,
    Pin,
    CopyName,
    ChangeUpstream,
    Archive,
    Delete,
}

/// The actions that make sense for `branch`. The current branch cannot be checked out,
/// merged into itself, rebased onto itself or deleted; remote branches are only read.
pub fn available_actions(branch: &BranchOverview) -> Vec<BranchAction> {
    use BranchAction::*;
    let local = branch.kind == BranchKind::Local;
    let other = !branch.is_head;
    [
        (Checkout, other),
        (Merge, other),
        (Rebase, other),
        (Rename, local),
        (Pin, true),
        (CopyName, true),
        (ChangeUpstream, local),
        (Archive, local),
        (Delete, local && other),
    ]
    .into_iter()
    .filter_map(|(action, shown)| shown.then_some(action))
    .collect()
}

/// The line under the merge / rebase button saying what the chosen action will do.
pub fn action_note(
    strategy: Option<MergeStrategy>,
    branch: &str,
    current: &str,
    ahead: usize,
) -> String {
    let names = [("branch", branch), ("current", current)];
    match strategy {
        Some(MergeStrategy::Merge) => {
            let count = ahead.to_string();
            tf(
                "branches_note_merge",
                &[names[0], names[1], ("count", &count)],
            )
        }
        Some(MergeStrategy::Rebase) => tf("branches_note_rebase", &names),
        None => String::new(),
    }
}

/// The selected branch against the current one: commits it has that the current branch
/// lacks, and the reverse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationCard {
    pub ahead: usize,
    pub behind: usize,
    pub against: String,
}

impl RelationCard {
    /// Another branch against the current one, or the current branch against its upstream;
    /// `None` for a current branch without an upstream.
    fn new(branch: &BranchOverview, current: &str) -> Option<Self> {
        if !branch.is_head {
            return Some(Self {
                ahead: branch.ahead_of_head,
                behind: branch.behind_head,
                against: current.to_string(),
            });
        }
        branch.upstream.as_ref().map(|upstream| Self {
            ahead: branch.upstream_ahead,
            behind: branch.upstream_behind,
            against: upstream.clone(),
        })
    }
}

/// The branch selected when none is: the first local branch other than the current one, in
/// list order with pinned branches first, or else the current branch.
fn initial_selection<'a>(branches: &'a [BranchOverview], pinned: &[String]) -> Option<&'a str> {
    let local = |branch: &&BranchOverview| branch.kind == BranchKind::Local;
    branches
        .iter()
        .filter(local)
        .filter(|branch| !branch.is_head)
        // min_by_key keeps the first of equal keys, so names stay in list order.
        .min_by_key(|branch| !pinned.contains(&branch.name))
        .or_else(|| branches.iter().filter(local).find(|branch| branch.is_head))
        .map(|branch| branch.name.as_str())
}

/// The remote branches of one remote, as its list group shows them.
#[derive(Debug)]
pub struct RemoteGroup<'a> {
    pub remote: &'a str,
    pub branches: Vec<&'a BranchOverview>,
}

/// Groups remote branches by remote, keeping their order; remotes come in the order their
/// first branch does.
pub fn remote_groups<'a>(branches: &[&'a BranchOverview]) -> Vec<RemoteGroup<'a>> {
    let mut groups: Vec<RemoteGroup<'a>> = Vec::new();
    for branch in branches.iter().filter(|b| b.kind == BranchKind::Remote) {
        // shortcut: the remote is the name up to the first '/', wrong for a remote whose own name has one; read repo.remotes() if that shows up.
        let remote = branch
            .name
            .split_once('/')
            .map_or(branch.name.as_str(), |(remote, _)| remote);
        match groups.iter_mut().find(|group| group.remote == remote) {
            Some(group) => group.branches.push(branch),
            None => groups.push(RemoteGroup {
                remote,
                branches: vec![branch],
            }),
        }
    }
    groups
}

/// A choice of the "Change upstream…" picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamChoice {
    None,
    Branch(String),
}

impl std::fmt::Display for UpstreamChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => f.write_str(t("branches_no_upstream_choice")),
            Self::Branch(name) => f.write_str(name),
        }
    }
}

/// The band open under the detail header: a name to type, an upstream to pick, or a
/// question to confirm. All but `NewBranch` act on the selected branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Band {
    NewBranch(String),
    Rename(String),
    Upstream(UpstreamChoice),
    Archive,
    Delete,
    Merge,
    Rebase,
}

/// How a git action ended.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// The success message.
    Done(String),
    /// A merge or rebase stopped on these files.
    Conflicts(Vec<String>),
}

#[derive(Debug, Clone)]
pub enum Message {
    Filter(String),
    /// Opens or closes the filter field's "⋯" menu.
    FilterMenu(bool),
    ToggleStaleOnly,
    /// Collapses or expands a remote's group.
    ToggleRemote(String),
    /// Shows a page of a remote's group.
    RemotePage(String, usize),
    Select(String),
    /// Opens or closes the selected branch's "⋯" menu.
    Menu(bool),
    Open(Band),
    CloseBand,
    BandText(String),
    UpstreamPicked(UpstreamChoice),
    /// Runs the action of the open band.
    Submit,
    /// A segment of "Merge into" | "Rebase onto": asks to confirm it.
    Strategy(usize),
    Checkout,
    TogglePin,
    CopyName,
    ViewInHistory,
    /// F2: renames the selected branch.
    StartRename,
    /// A read of the branches and of HEAD, numbered so that only the latest one is shown.
    Loaded(Arc<str>, u64, Result<BranchList, String>),
    /// The commits of a branch that the current one lacks, or the latest ones of the current
    /// branch.
    CommitsLoaded(Arc<str>, String, Result<Vec<CommitInfo>, String>),
    /// The key names the failure title.
    ActionDone(Arc<str>, &'static str, Result<Outcome, String>),
}

/// The branches and the commit HEAD points at.
pub type BranchList = (Vec<BranchOverview>, Option<Oid>);

pub struct BranchPanel {
    repo: Option<Arc<str>>,
    branches: Vec<BranchOverview>,
    /// The commit HEAD pointed at when `branches` was read.
    head: Option<Oid>,
    loading: bool,
    error: Option<String>,
    /// Number of the last branch read; older replies are dropped.
    request: u64,
    filter: String,
    filter_menu: bool,
    stale_only: bool,
    collapsed: HashSet<String>,
    remote_pages: HashMap<String, usize>,
    selected: Option<String>,
    commits: Vec<CommitInfo>,
    commits_error: Option<String>,
    menu: bool,
    strategy: MergeStrategy,
    band: Option<Band>,
    busy: bool,
    pinned_branches: Vec<String>,
    pending_pinned: Option<Vec<String>>,
    open_history_branch: Option<String>,
}

impl BranchPanel {
    pub fn new(pinned_branches: Vec<String>) -> Self {
        Self {
            repo: None,
            branches: Vec::new(),
            head: None,
            loading: false,
            error: None,
            request: 0,
            filter: String::new(),
            filter_menu: false,
            stale_only: false,
            collapsed: HashSet::new(),
            remote_pages: HashMap::new(),
            selected: None,
            commits: Vec::new(),
            commits_error: None,
            menu: false,
            strategy: MergeStrategy::Merge,
            band: None,
            busy: false,
            pinned_branches,
            pending_pinned: None,
            open_history_branch: None,
        }
    }

    /// Reads the branches of `repo` in the background. Another repository starts from an
    /// empty panel with the pinned branches kept. The open one keeps everything, an open band
    /// and a running action included: the window regaining focus reloads it.
    pub fn load(&mut self, repo: &RepoContext) -> Task<Message> {
        if !self.is_current(repo.path()) {
            let pinned = std::mem::take(&mut self.pinned_branches);
            // Keep request numbers growing so a reply from before the reset is dropped.
            let request = self.request;
            *self = Self::new(pinned);
            self.repo = Some(repo.path_arc());
            self.request = request;
        }
        self.refresh()
    }

    pub fn set_pinned_branches(&mut self, pinned_branches: Vec<String>) {
        self.pinned_branches = pinned_branches;
    }

    /// The pinned branches after a pin or unpin, once.
    pub fn take_pinned_changes(&mut self) -> Option<Vec<String>> {
        self.pending_pinned.take()
    }

    /// The branch to show in History, once.
    pub fn take_history_request(&mut self) -> Option<String> {
        self.open_history_branch.take()
    }

    /// Handles a message; the toast, if any, is for the application to show.
    pub fn update(&mut self, message: Message) -> (Task<Message>, Option<Toast>) {
        match message {
            Message::Filter(value) => self.filter = value,
            Message::FilterMenu(open) => self.filter_menu = open,
            Message::ToggleStaleOnly => {
                self.filter_menu = false;
                self.stale_only = !self.stale_only;
            }
            Message::ToggleRemote(remote) => {
                if !self.collapsed.remove(&remote) {
                    self.collapsed.insert(remote);
                }
            }
            Message::RemotePage(remote, page) => {
                self.remote_pages.insert(remote, page);
            }
            Message::Select(name) => return (self.select(name), None),
            Message::Menu(open) => self.menu = open,
            Message::Open(band) => {
                self.menu = false;
                self.band = Some(band);
            }
            Message::CloseBand => self.band = None,
            Message::BandText(value) => {
                if let Some(Band::NewBranch(name) | Band::Rename(name)) = &mut self.band {
                    *name = value;
                }
            }
            Message::UpstreamPicked(choice) => {
                if let Some(Band::Upstream(picked)) = &mut self.band {
                    *picked = choice;
                }
            }
            Message::Submit => return (self.submit(), None),
            Message::Strategy(index) => {
                self.strategy = if index == 0 {
                    MergeStrategy::Merge
                } else {
                    MergeStrategy::Rebase
                };
                self.band = Some(match self.strategy {
                    MergeStrategy::Merge => Band::Merge,
                    MergeStrategy::Rebase => Band::Rebase,
                });
            }
            Message::Checkout => return (self.checkout(), None),
            Message::TogglePin => {
                self.menu = false;
                if let Some(name) = self.selected.clone() {
                    match self.pinned_branches.iter().position(|n| *n == name) {
                        Some(at) => {
                            self.pinned_branches.remove(at);
                        }
                        None => self.pinned_branches.push(name),
                    }
                    self.pending_pinned = Some(self.pinned_branches.clone());
                }
            }
            Message::CopyName => {
                self.menu = false;
                if let Some(name) = self.selected.clone() {
                    return (iced::clipboard::write(name), None);
                }
            }
            Message::ViewInHistory => self.open_history_branch = self.selected.clone(),
            Message::StartRename => {
                let local = self
                    .selected_branch()
                    .filter(|branch| branch.kind == BranchKind::Local)
                    .map(|branch| branch.name.clone());
                if let Some(name) = local {
                    self.menu = false;
                    self.band = Some(Band::Rename(name));
                }
            }
            Message::Loaded(repo, request, result) => {
                if self.is_current(&repo) && request == self.request {
                    return (self.apply_list(result), None);
                }
            }
            Message::CommitsLoaded(repo, name, result) => {
                if self.is_current(&repo) && self.selected.as_ref() == Some(&name) {
                    match result {
                        Ok(commits) => self.commits = commits,
                        Err(err) => self.commits_error = Some(err),
                    }
                }
            }
            Message::ActionDone(repo, failed, result) => {
                if self.is_current(&repo) {
                    self.busy = false;
                    let toast = match result {
                        Ok(Outcome::Done(message)) => Toast::success(message),
                        Ok(Outcome::Conflicts(files)) => Toast::error(
                            t("branches_conflicts"),
                            tf("branches_conflicts_detail", &[("files", &files.join(", "))]),
                        ),
                        Err(err) => Toast::error(t(failed), err),
                    };
                    return (self.refresh(), Some(toast));
                }
            }
        }
        (Task::none(), None)
    }

    fn is_current(&self, repo: &str) -> bool {
        self.repo.as_deref() == Some(repo)
    }

    fn selected_branch(&self) -> Option<&BranchOverview> {
        let name = self.selected.as_ref()?;
        self.branches.iter().find(|branch| &branch.name == name)
    }

    /// The checked-out branch, or "HEAD" when none is.
    fn current_name(&self) -> &str {
        head_name(&self.branches)
    }

    fn is_pinned(&self, name: &str) -> bool {
        self.pinned_branches.iter().any(|pinned| pinned == name)
    }

    fn refresh(&mut self) -> Task<Message> {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        self.request += 1;
        self.loading = true;
        let request = self.request;
        git_task(repo, read_branches, move |repo, result| {
            Message::Loaded(repo, request, result)
        })
    }

    /// Takes the new list; when the selected branch is gone, or nothing is selected yet, the
    /// selection moves to `initial_selection`.
    fn apply_list(&mut self, result: Result<BranchList, String>) -> Task<Message> {
        self.loading = false;
        match result {
            Ok((branches, head)) => {
                self.error = None;
                self.branches = branches;
                self.head = head;
                let name = match self.selected_branch() {
                    Some(branch) => Some(branch.name.clone()),
                    None => {
                        self.band = None;
                        self.menu = false;
                        initial_selection(&self.branches, &self.pinned_branches).map(str::to_string)
                    }
                };
                self.selected = name;
                // The tips may have moved: the own commits are read again.
                self.load_commits()
            }
            Err(err) => {
                self.error = Some(err);
                Task::none()
            }
        }
    }

    fn select(&mut self, name: String) -> Task<Message> {
        if self.selected.as_ref() != Some(&name) {
            self.menu = false;
            // A new branch name typed in the band does not belong to the old selection.
            if !matches!(self.band, Some(Band::NewBranch(_))) {
                self.band = None;
            }
        }
        self.selected = Some(name);
        self.load_commits()
    }

    /// Reads the commits listed under the selected branch.
    fn load_commits(&mut self) -> Task<Message> {
        self.commits.clear();
        self.commits_error = None;
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        let Some(branch) = self.selected_branch() else {
            return Task::none();
        };
        let (name, is_head) = (branch.name.clone(), branch.is_head);
        let job_name = name.clone();
        git_task(
            repo,
            move |repo| read_commits(repo, &job_name, is_head),
            move |repo, result| Message::CommitsLoaded(repo, name.clone(), result),
        )
    }

    /// Checks out the selected branch. A remote branch is checked out through its local
    /// branch, created to track it when there is none yet.
    fn checkout(&mut self) -> Task<Message> {
        let Some(branch) = self.selected_branch() else {
            return Task::none();
        };
        let name = branch.name.clone();
        if branch.kind == BranchKind::Local {
            let done = tf("branches_checked_out", &[("name", &name)]);
            return self.run("branches_checkout_failed", move |repo| {
                checkout_branch(repo, &name)
                    .map(|()| Outcome::Done(done))
                    .map_err(|err| err.to_string())
            });
        }
        let local = name.split_once('/').map_or(name.as_str(), |(_, rest)| rest);
        let exists = self
            .branches
            .iter()
            .any(|b| b.kind == BranchKind::Local && b.name == local);
        let local = local.to_string();
        self.run("branches_checkout_failed", move |repo| {
            let local = if exists {
                local
            } else {
                create_tracking_branch(repo, &name).map_err(|err| err.to_string())?
            };
            checkout_branch(repo, &local)
                .map(|()| Outcome::Done(tf("branches_checked_out", &[("name", &local)])))
                .map_err(|err| err.to_string())
        })
    }

    /// Runs the action of the open band.
    fn submit(&mut self) -> Task<Message> {
        if self.busy {
            return Task::none();
        }
        let Some(band) = self.band.take() else {
            return Task::none();
        };
        if let Band::NewBranch(name) = band {
            let name = name.trim().to_string();
            if name.is_empty() {
                self.band = Some(Band::NewBranch(name));
                return Task::none();
            }
            let done = tf("branches_created", &[("name", &name)]);
            self.selected = Some(name.clone());
            return self.run("branches_create_failed", move |repo| {
                create_branch(repo, &name, None)
                    .map(|()| Outcome::Done(done))
                    .map_err(|err| err.to_string())
            });
        }
        let Some(name) = self.selected.clone() else {
            return Task::none();
        };
        let current = self.current_name().to_string();
        let head = self.head;
        match band {
            Band::NewBranch(_) => Task::none(),
            Band::Rename(new) => {
                let new = new.trim().to_string();
                if new.is_empty() || new == name {
                    return Task::none();
                }
                let done = tf("branches_renamed", &[("old", &name), ("new", &new)]);
                self.selected = Some(new.clone());
                self.run("branches_rename_failed", move |repo| {
                    rename_branch(repo, &name, &new)
                        .map(|()| Outcome::Done(done))
                        .map_err(|err| err.to_string())
                })
            }
            Band::Upstream(choice) => {
                let upstream = match choice {
                    UpstreamChoice::None => None,
                    UpstreamChoice::Branch(upstream) => Some(upstream),
                };
                let done = match &upstream {
                    Some(upstream) => tf(
                        "branches_upstream_set",
                        &[("branch", &name), ("upstream", upstream)],
                    ),
                    None => tf("branches_upstream_removed", &[("branch", &name)]),
                };
                self.run("branches_upstream_failed", move |repo| {
                    set_upstream(repo, &name, upstream.as_deref())
                        .map(|()| Outcome::Done(done))
                        .map_err(|err| err.to_string())
                })
            }
            Band::Archive => self.run("branches_archive_failed", move |repo| {
                archive_branch(repo, &name)
                    .map(|tag| {
                        Outcome::Done(tf("branches_archived", &[("branch", &name), ("tag", &tag)]))
                    })
                    .map_err(|err| err.to_string())
            }),
            Band::Delete => {
                let done = tf("branches_deleted", &[("name", &name)]);
                let merged = self
                    .selected_branch()
                    .is_some_and(|branch| branch.merged_into_head);
                self.run("branches_delete_failed", move |repo| {
                    ensure_unchanged(repo, &current, head, &name, merged)?;
                    delete_branch(repo, &name)
                        .map(|()| Outcome::Done(done))
                        .map_err(|err| err.to_string())
                })
            }
            Band::Merge | Band::Rebase => {
                let (strategy, done, failed) = if band == Band::Merge {
                    (
                        MergeStrategy::Merge,
                        "branches_merged_into",
                        "branches_merge_failed",
                    )
                } else {
                    (
                        MergeStrategy::Rebase,
                        "branches_rebased",
                        "branches_rebase_failed",
                    )
                };
                let done = tf(done, &[("branch", &name), ("current", &current)]);
                self.run(failed, move |repo| {
                    ensure_unchanged(repo, &current, head, &name, false)?;
                    let outcome = merge_branch(repo, &name, strategy)?;
                    Ok(if outcome.had_conflicts {
                        Outcome::Conflicts(outcome.conflicts)
                    } else {
                        Outcome::Done(done)
                    })
                })
            }
        }
    }

    /// Runs a git action unless one is running; the list is read again when it ends.
    fn run(
        &mut self,
        failed: &'static str,
        job: impl FnOnce(&str) -> Result<Outcome, String> + Send + 'static,
    ) -> Task<Message> {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        if self.busy {
            return Task::none();
        }
        self.busy = true;
        git_task(repo, job, move |repo, result| {
            Message::ActionDone(repo, failed, result)
        })
    }

    pub fn view(&self) -> Element<'_, Message> {
        let list = column![self.toolbar(), self.branch_list()]
            .width(Length::Fixed(LIST_WIDTH))
            .height(Length::Fill);
        row![list, vertical_divider(), self.detail_pane()]
            .height(Length::Fill)
            .into()
    }

    /// The filter field with its "⋯" (stale only), and "+ New".
    fn toolbar(&self) -> Element<'_, Message> {
        let stale = if self.stale_only {
            t("branches_show_all")
        } else {
            t("branches_stale_only")
        };
        let options = menu::popup(
            gs_button::icon(
                MORE_GLYPH,
                Some(Message::FilterMenu(!self.filter_menu)),
                t("branches_filter_options"),
            ),
            self.filter_menu,
            vec![item(stale, Message::ToggleStaleOnly, false)],
            Message::FilterMenu(false),
        );
        let filter = field(
            text_input(t("branches_filter_placeholder"), &self.filter).on_input(Message::Filter),
            Some(options),
        );
        let new = gs_button::secondary(
            t("branches_new"),
            Some(Message::Open(Band::NewBranch(String::new()))),
        );
        container(row![filter, new].spacing(6).align_y(Alignment::Center))
            .padding([10, 10])
            .into()
    }

    fn branch_list(&self) -> Element<'_, Message> {
        let now = chrono::Utc::now().timestamp();
        let query = self.filter.trim().to_lowercase();
        let visible: Vec<&BranchOverview> = self
            .branches
            .iter()
            .filter(|branch| query.is_empty() || branch.name.to_lowercase().contains(&query))
            .filter(|branch| !self.stale_only || is_stale(branch, now))
            .collect();
        if visible.is_empty() {
            let label = match &self.error {
                Some(err) => tf("branches_load_failed", &[("error", err)]),
                None if self.loading => t("changes_loading").to_string(),
                None if !self.branches.is_empty() => t("branches_no_match").to_string(),
                None => t("branches_empty").to_string(),
            };
            return note(label);
        }

        let mut local: Vec<&BranchOverview> = visible
            .iter()
            .copied()
            .filter(|branch| branch.kind == BranchKind::Local)
            .collect();
        // Pinned branches first; the sort is stable, so names stay in order.
        local.sort_by_key(|branch| !self.is_pinned(&branch.name));
        let mut rows = Column::new().padding([0, 6]).spacing(1);
        if !local.is_empty() {
            rows = rows.push(group_header(
                tf(
                    "branches_local_group",
                    &[("count", &local.len().to_string())],
                ),
                None,
            ));
            for branch in local {
                rows = rows.push(self.branch_row(branch));
            }
        }
        for group in remote_groups(&visible) {
            let collapsed = self.collapsed.contains(group.remote);
            let label = format!("{} · {}", group.remote.to_uppercase(), group.branches.len());
            rows = rows.push(group_header(
                label,
                Some((collapsed, Message::ToggleRemote(group.remote.to_string()))),
            ));
            if collapsed {
                continue;
            }
            let pages = group.branches.len().div_ceil(REMOTE_PAGE_SIZE);
            let page = self
                .remote_pages
                .get(group.remote)
                .copied()
                .unwrap_or(0)
                .min(pages - 1);
            for branch in group
                .branches
                .iter()
                .skip(page * REMOTE_PAGE_SIZE)
                .take(REMOTE_PAGE_SIZE)
            {
                rows = rows.push(self.branch_row(branch));
            }
            if pages > 1 {
                rows = rows.push(pager(group.remote, page, pages));
            }
        }
        scrollable(rows.padding([0, 6])).height(Length::Fill).into()
    }

    /// The icon, the name, and on the right: "current", "✓ merged" or ↑↓ against the
    /// current branch.
    fn branch_row<'a>(&'a self, branch: &'a BranchOverview) -> Element<'a, Message> {
        let typo = Typography::current();
        let (glyph, color) = if branch.is_head {
            (CURRENT_GLYPH, Palette::SUCCESS())
        } else if self.is_pinned(&branch.name) {
            (STAR_GLYPH, Palette::WARNING())
        } else if branch.kind == BranchKind::Remote {
            (CLOUD_GLYPH, Palette::TEXT_DIMMER())
        } else {
            (BRANCH_GLYPH, Palette::TEXT_DIMMER())
        };
        let small = |label: String, color| {
            text(label)
                .size(typo.sz(10))
                .font(typo.regular)
                .color(color)
        };
        let mut content = row![
            container(
                text(glyph)
                    .size(typo.sz(10))
                    .font(fonts::glyph_font())
                    .color(color)
            )
            .center_x(Length::Fixed(14.0)),
            container(
                text(branch.name.as_str())
                    .size(typo.sz(12))
                    .font(typo.regular)
                    .color(Palette::TEXT_PRIMARY())
                    .wrapping(Wrapping::None),
            )
            .width(Length::Fill)
            .clip(true),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if branch.is_head {
            content = content.push(small(
                t("branches_current").to_string(),
                Palette::TEXT_DIMMER(),
            ));
        } else if branch.kind == BranchKind::Local && branch.merged_into_head {
            content = content.push(
                row![
                    text(icons::CHECK)
                        .size(typo.sz(9))
                        .font(fonts::glyph_font())
                        .color(Palette::SUCCESS()),
                    small(t("branches_merged").to_string(), Palette::SUCCESS()),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            );
        } else if branch.kind == BranchKind::Local {
            if branch.ahead_of_head > 0 {
                content = content.push(small(
                    format!("↑{}", branch.ahead_of_head),
                    Palette::SUCCESS(),
                ));
            }
            if branch.behind_head > 0 {
                content = content.push(small(
                    format!("↓{}", branch.behind_head),
                    Palette::WARNING(),
                ));
            }
        }
        let selected = self.selected.as_ref() == Some(&branch.name);
        button(content)
            .on_press(Message::Select(branch.name.clone()))
            .padding([6, 8])
            .width(Length::Fill)
            .style(move |_, status| button::Style {
                background: (selected
                    || matches!(status, button::Status::Hovered | button::Status::Pressed))
                .then(|| Palette::BG_CARD_HOVER().into()),
                border: Border::default().rounded(6),
                ..Default::default()
            })
            .into()
    }

    fn detail_pane(&self) -> Element<'_, Message> {
        let Some(branch) = self.selected_branch() else {
            return column![self.bands(None), note(t("branches_select").to_string())].into();
        };
        let actions = available_actions(branch);
        let current = self.current_name();
        let mut body = Column::new().spacing(14).padding([16, 18]);
        if let Some(card) = RelationCard::new(branch, current) {
            body = body.push(relation_card(&card));
        }
        if actions.contains(&BranchAction::Checkout) {
            let mut buttons = row![gs_button::primary(
                t("branches_checkout"),
                (!self.busy).then_some(Message::Checkout),
            )]
            .spacing(10)
            .align_y(Alignment::Center);
            if actions.contains(&BranchAction::Merge) {
                let segments = [
                    tf("branches_merge_into", &[("current", current)]),
                    tf("branches_rebase_onto", &[("current", current)]),
                ];
                let chosen = usize::from(self.strategy == MergeStrategy::Rebase);
                buttons = buttons.push(segmented(&segments, chosen, Message::Strategy));
            }
            body = body.push(column![buttons, self.note_line(branch, current)].spacing(8));
        }
        body = body.push(self.own_commits(branch, current));
        column![
            self.header(branch, &actions),
            self.bands(Some(branch)),
            horizontal_divider(),
            scrollable(body).height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn note_line<'a>(&self, branch: &BranchOverview, current: &str) -> Element<'a, Message> {
        let typo = Typography::current();
        let note = action_note(
            Some(self.strategy),
            &branch.name,
            current,
            branch.ahead_of_head,
        );
        text(note)
            .size(typo.sz(11))
            .font(typo.regular)
            .color(Palette::TEXT_DIM())
            .into()
    }

    /// The name, "tracks … · in sync · last commit …", and the "⋯" menu.
    fn header<'a>(
        &'a self,
        branch: &'a BranchOverview,
        actions: &[BranchAction],
    ) -> Element<'a, Message> {
        let typo = Typography::current();
        let more = menu::popup(
            gs_button::icon(
                MORE_GLYPH,
                Some(Message::Menu(!self.menu)),
                t("branches_actions"),
            ),
            self.menu,
            self.menu_entries(branch, actions),
            Message::Menu(false),
        );
        column![
            row![
                text(branch.name.as_str())
                    .size(typo.sz(15))
                    .font(typo.bold)
                    .color(Palette::TEXT_PRIMARY())
                    .width(Length::Fill),
                more,
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            text(subtitle(branch, chrono::Utc::now().timestamp()))
                .size(typo.sz(11))
                .font(typo.regular)
                .color(Palette::TEXT_DIM()),
        ]
        .spacing(6)
        .padding([16, 18])
        .into()
    }

    /// Rename (F2), Pin to top, Copy name, Change upstream…, then Archive as tag… and Delete….
    fn menu_entries(
        &self,
        branch: &BranchOverview,
        actions: &[BranchAction],
    ) -> Vec<Entry<Message>> {
        let mut entries = Vec::new();
        for action in actions {
            let entry = match action {
                BranchAction::Checkout | BranchAction::Merge | BranchAction::Rebase => continue,
                BranchAction::Rename => Entry::Item(MenuItem {
                    label: t("branches_rename").to_string(),
                    shortcut: Some("F2".to_string()),
                    on_select: Message::Open(Band::Rename(branch.name.clone())),
                    danger: false,
                }),
                BranchAction::Pin => {
                    let label = if self.is_pinned(&branch.name) {
                        t("branches_unpin")
                    } else {
                        t("branches_pin")
                    };
                    item(label, Message::TogglePin, false)
                }
                BranchAction::CopyName => item(t("branches_copy_name"), Message::CopyName, false),
                BranchAction::ChangeUpstream => {
                    let choice = branch
                        .upstream
                        .clone()
                        .map_or(UpstreamChoice::None, UpstreamChoice::Branch);
                    item(
                        t("branches_change_upstream"),
                        Message::Open(Band::Upstream(choice)),
                        false,
                    )
                }
                BranchAction::Archive => {
                    entries.push(Entry::Separator);
                    item(t("branches_archive"), Message::Open(Band::Archive), false)
                }
                BranchAction::Delete => {
                    if !actions.contains(&BranchAction::Archive) {
                        entries.push(Entry::Separator);
                    }
                    item(t("branches_delete"), Message::Open(Band::Delete), true)
                }
            };
            entries.push(entry);
        }
        entries
    }

    /// The open band: a name field, the upstream picker or a confirmation.
    fn bands<'a>(&'a self, branch: Option<&'a BranchOverview>) -> Element<'a, Message> {
        let typo = Typography::current();
        let Some(band) = &self.band else {
            return Space::new().into();
        };
        let prompt = |label: String| -> Element<'a, Message> {
            text(label)
                .size(typo.sz(12))
                .font(typo.regular)
                .color(Palette::TEXT_SECONDARY())
                .into()
        };
        let current = self.current_name();
        let submit = (!self.busy).then_some(Message::Submit);
        let name_field = |value: &'a String| -> Element<'a, Message> {
            container(field(
                text_input(t("branches_name_placeholder"), value)
                    .on_input(Message::BandText)
                    .on_submit_maybe(
                        (!self.busy && !value.trim().is_empty()).then_some(Message::Submit),
                    ),
                None,
            ))
            .width(Length::Fixed(220.0))
            .into()
        };
        let (lines, action) = match (band, branch) {
            (Band::NewBranch(name), _) => (
                vec![
                    prompt(tf("branches_new_prompt", &[("current", current)])),
                    name_field(name),
                ],
                gs_button::primary(
                    t("branches_create"),
                    submit.filter(|_| !name.trim().is_empty()),
                ),
            ),
            (_, None) => return Space::new().into(),
            (Band::Rename(name), Some(branch)) => (
                vec![
                    prompt(tf("branches_rename_prompt", &[("branch", &branch.name)])),
                    name_field(name),
                ],
                gs_button::primary(
                    t("branches_rename_button"),
                    submit.filter(|_| !name.trim().is_empty() && name.trim() != branch.name),
                ),
            ),
            (Band::Upstream(choice), Some(branch)) => {
                let choices: Vec<UpstreamChoice> = std::iter::once(UpstreamChoice::None)
                    .chain(
                        self.branches
                            .iter()
                            .filter(|b| b.kind == BranchKind::Remote)
                            .map(|b| UpstreamChoice::Branch(b.name.clone())),
                    )
                    .collect();
                (
                    vec![
                        prompt(tf("branches_upstream_prompt", &[("branch", &branch.name)])),
                        pick_list(choices, Some(choice.clone()), Message::UpstreamPicked)
                            .text_size(typo.sz(12))
                            .font(typo.regular)
                            .into(),
                    ],
                    gs_button::primary(t("branches_apply"), submit),
                )
            }
            (Band::Archive, Some(branch)) => (
                vec![prompt(tf(
                    "branches_archive_confirm",
                    &[("branch", &branch.name)],
                ))],
                gs_button::primary(t("branches_archive_button"), submit),
            ),
            (Band::Delete, Some(branch)) => {
                let key = if branch.merged_into_head {
                    "branches_delete_merged"
                } else {
                    "branches_delete_unmerged"
                };
                (
                    vec![prompt(tf(
                        key,
                        &[("branch", &branch.name), ("current", current)],
                    ))],
                    gs_button::danger(t("branches_delete_button"), submit),
                )
            }
            (Band::Merge, Some(branch)) => (
                vec![prompt(tf(
                    "branches_merge_confirm",
                    &[("branch", &branch.name), ("current", current)],
                ))],
                gs_button::primary(t("branches_merge_button"), submit),
            ),
            (Band::Rebase, Some(branch)) => (
                vec![prompt(tf(
                    "branches_rebase_confirm",
                    &[("branch", &branch.name), ("current", current)],
                ))],
                gs_button::primary(t("branches_rebase_button"), submit),
            ),
        };
        question(lines, action, Message::CloseBand)
    }

    /// "Commits only on <branch>", or "Latest commits on <branch>" for the current branch,
    /// with "View in History →", then the commits.
    fn own_commits<'a>(
        &'a self,
        branch: &'a BranchOverview,
        current: &str,
    ) -> Element<'a, Message> {
        let typo = Typography::current();
        let link = button(
            text(t("branches_view_in_history"))
                .size(typo.sz(11))
                .font(typo.regular)
                .color(Palette::ACCENT()),
        )
        .padding(0)
        .style(|_, _| button::Style::default())
        .on_press(Message::ViewInHistory);
        let heading = if branch.is_head {
            "branches_latest_commits"
        } else {
            "branches_commits_only_on"
        };
        let heading = row![
            text(tf(heading, &[("branch", &branch.name)]))
                .size(typo.sz(12))
                .font(typo.bold)
                .color(Palette::TEXT_SECONDARY())
                .width(Length::Fill),
            link,
        ]
        .align_y(Alignment::Center);
        let mut list = Column::new().spacing(1);
        if let Some(err) = &self.commits_error {
            list = list.push(note_line(tf("branches_commits_failed", &[("error", err)])));
        } else if self.commits.is_empty() && !branch.is_head && branch.ahead_of_head == 0 {
            list = list.push(note_line(tf(
                "branches_no_own_commits",
                &[("branch", &branch.name), ("current", current)],
            )));
        }
        let now = chrono::Utc::now().timestamp();
        for commit in &self.commits {
            list = list.push(
                row![
                    text(commit.short_id.as_str())
                        .size(typo.sz(11))
                        .font(typo.regular)
                        .color(Palette::ACCENT()),
                    container(
                        text(commit.summary.as_str())
                            .size(typo.sz(12))
                            .font(typo.regular)
                            .color(Palette::TEXT_PRIMARY())
                            .wrapping(Wrapping::None),
                    )
                    .width(Length::Fill)
                    .clip(true),
                    text(relative_time(commit.time.seconds(), now))
                        .size(typo.sz(11))
                        .font(typo.regular)
                        .color(Palette::TEXT_DIMMEST()),
                ]
                .spacing(10)
                .padding([5, 8])
                .align_y(Alignment::Center),
            );
        }
        column![heading, list].spacing(8).into()
    }
}

/// "tracks origin/x · in sync with its upstream · last commit 1 h ago".
fn subtitle(branch: &BranchOverview, now: i64) -> String {
    let mut parts = Vec::new();
    if branch.kind == BranchKind::Remote {
        parts.push(t("branches_remote_branch").to_string());
    } else if let Some(upstream) = &branch.upstream {
        parts.push(tf("branches_tracks", &[("upstream", upstream)]));
        let (ahead, behind) = (branch.upstream_ahead, branch.upstream_behind);
        parts.push(match (ahead, behind) {
            (0, 0) => t("branches_in_sync").to_string(),
            (ahead, 0) => tf("branches_upstream_ahead", &[("count", &ahead.to_string())]),
            (0, behind) => tf(
                "branches_upstream_behind",
                &[("count", &behind.to_string())],
            ),
            (ahead, behind) => tf(
                "branches_upstream_diverged",
                &[
                    ("ahead", &ahead.to_string()),
                    ("behind", &behind.to_string()),
                ],
            ),
        });
    } else {
        parts.push(t("branches_no_upstream").to_string());
    }
    if let Some(time) = branch.last_commit_time {
        parts.push(tf(
            "branches_last_commit",
            &[("when", &relative_time(time, now))],
        ));
    }
    parts.join(" · ")
}

/// No commit for `STALE_DAYS`, or no commit at all.
fn is_stale(branch: &BranchOverview, now: i64) -> bool {
    branch
        .last_commit_time
        .is_none_or(|time| now.saturating_sub(time) > STALE_DAYS * 24 * 60 * 60)
}

/// Big "N commits ahead" on the left, "N behind <current>" on the right, a proportional
/// bar between.
fn relation_card<'a>(card: &RelationCard) -> Element<'a, Message> {
    let typo = Typography::current();
    let side = |count: usize, label: String, color, align| {
        column![
            text(count.to_string())
                .size(typo.sz(22))
                .font(typo.bold)
                .color(color),
            text(label)
                .size(typo.sz(11))
                .font(typo.regular)
                .color(Palette::TEXT_DIM()),
        ]
        .spacing(2)
        .align_x(align)
    };
    let ahead_label = if card.ahead == 1 {
        t("branches_commit_ahead").to_string()
    } else {
        t("branches_commits_ahead").to_string()
    };
    let segment = |portion: usize, color: iced::Color| -> Element<'a, Message> {
        let portion = u16::try_from(portion).unwrap_or(u16::MAX);
        container(Space::new().width(Length::Fill).height(6))
            .width(Length::FillPortion(portion))
            .style(move |_| container::Style {
                background: Some(color.into()),
                border: Border::default().rounded(3),
                ..Default::default()
            })
            .into()
    };
    let mut bar = row![].spacing(2).width(Length::Fill);
    if card.ahead == 0 && card.behind == 0 {
        bar = bar.push(segment(1, Palette::BG_SELECTED()));
    }
    if card.ahead > 0 {
        bar = bar.push(segment(card.ahead, Palette::SUCCESS()));
    }
    if card.behind > 0 {
        bar = bar.push(segment(card.behind, Palette::WARNING()));
    }
    container(
        row![
            side(
                card.ahead,
                ahead_label,
                Palette::SUCCESS(),
                Alignment::Start
            ),
            container(bar).width(Length::Fill).padding([0, 16]),
            side(
                card.behind,
                tf("branches_behind", &[("branch", &card.against)]),
                Palette::WARNING(),
                Alignment::End,
            ),
        ]
        .align_y(Alignment::Center),
    )
    .padding([14, 16])
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Palette::BG_CARD().into()),
        border: Border::default().rounded(8),
        ..Default::default()
    })
    .into()
}

/// "LOCAL · 3", or a remote's "ORIGIN · 4" with its collapse chevron.
fn group_header<'a>(label: String, toggle: Option<(bool, Message)>) -> Element<'a, Message> {
    let typo = Typography::current();
    let mut content = row![text(label)
        .size(typo.sz(10))
        .font(typo.regular)
        .color(Palette::TEXT_DIMMEST())
        .width(Length::Fill)]
    .align_y(Alignment::Center);
    let Some((collapsed, on_press)) = toggle else {
        return container(content).padding([10, 8]).into();
    };
    content = content.push(
        text(if collapsed {
            icons::CHEVRON_RIGHT
        } else {
            icons::CHEVRON_DOWN
        })
        .size(typo.sz(9))
        .font(fonts::glyph_font())
        .color(Palette::TEXT_DIMMEST()),
    );
    button(content)
        .on_press(on_press)
        .padding([10, 8])
        .width(Length::Fill)
        .style(|_, _| button::Style::default())
        .into()
}

/// "‹ 2/3 ›" under a remote group longer than a page.
fn pager<'a>(remote: &str, page: usize, pages: usize) -> Element<'a, Message> {
    let typo = Typography::current();
    let go = |to: usize| Message::RemotePage(remote.to_string(), to);
    container(
        row![
            gs_button::icon(
                PREVIOUS_GLYPH,
                (page > 0).then(|| go(page - 1)),
                t("branches_previous_page"),
            ),
            text(format!("{}/{}", page + 1, pages))
                .size(typo.sz(11))
                .font(typo.regular)
                .color(Palette::TEXT_DIMMER()),
            gs_button::icon(
                icons::CHEVRON_RIGHT,
                (page + 1 < pages).then(|| go(page + 1)),
                t("branches_next_page"),
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .center_x(Length::Fill)
    .padding([4, 0])
    .into()
}

/// The latest commits of the current branch, or the commits of another branch that the
/// current one lacks.
fn read_commits(repo: &str, branch: &str, is_head: bool) -> Result<Vec<CommitInfo>, String> {
    let commits = if is_head {
        let filter = CommitFilter {
            branch: Some(branch.to_string()),
            ..CommitFilter::default()
        };
        read_commit_log(repo, &filter, LATEST_COMMITS, false)
    } else {
        commits_between_refs(repo, "HEAD", branch, OWN_COMMITS)
    };
    commits.map_err(|err| err.to_string())
}

/// The checked-out branch, or "HEAD" when none is.
fn head_name(branches: &[BranchOverview]) -> &str {
    branches
        .iter()
        .find(|branch| branch.is_head && branch.kind == BranchKind::Local)
        .map_or("HEAD", |branch| branch.name.as_str())
}

/// The branches and the commit HEAD points at, read in a background task.
fn read_branches(repo: &str) -> Result<BranchList, String> {
    let branches = list_branch_overview(repo).map_err(|err| err.to_string())?;
    let head = head_commit(repo).map_err(|err| err.to_string())?;
    Ok((branches, head))
}

/// Fails when HEAD is no longer `current` at commit `head`, or when `branch` was shown as
/// merged into it and no longer is: the confirmation the user answered then describes another
/// state. The commit matters for a detached HEAD, whose name is always "HEAD".
fn ensure_unchanged(
    repo: &str,
    current: &str,
    head: Option<Oid>,
    branch: &str,
    merged: bool,
) -> Result<(), String> {
    let (branches, now) = read_branches(repo)?;
    let still_merged = branches.iter().any(|overview| {
        overview.kind == BranchKind::Local && overview.name == branch && overview.merged_into_head
    });
    if head_name(&branches) != current || now != head || (merged && !still_merged) {
        return Err(t("branches_changed_outside").to_string());
    }
    Ok(())
}

fn note<'a>(label: String) -> Element<'a, Message> {
    container(note_line(label))
        .padding([12, 16])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn note_line<'a>(label: String) -> Element<'a, Message> {
    let typo = Typography::current();
    text(label)
        .size(typo.sz(12))
        .font(typo.regular)
        .color(Palette::TEXT_DIMMER())
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overview(name: &str, kind: BranchKind, is_head: bool) -> BranchOverview {
        BranchOverview {
            name: name.to_string(),
            kind,
            is_head,
            upstream: None,
            upstream_ahead: 0,
            upstream_behind: 0,
            ahead_of_head: 0,
            behind_head: 0,
            merged_into_head: false,
            last_commit_time: None,
        }
    }

    #[test]
    fn action_note_describes_merge_and_rebase() {
        let merge = action_note(Some(MergeStrategy::Merge), "feature/badges", "main", 3);
        assert_eq!(
            merge,
            tf(
                "branches_note_merge",
                &[
                    ("branch", "feature/badges"),
                    ("current", "main"),
                    ("count", "3")
                ]
            )
        );
        assert!(merge.contains("feature/badges") && merge.contains("main"));
        assert!(merge.contains('3'));

        let rebase = action_note(Some(MergeStrategy::Rebase), "feature/badges", "main", 3);
        assert_eq!(
            rebase,
            tf(
                "branches_note_rebase",
                &[("branch", "feature/badges"), ("current", "main")]
            )
        );
        assert!(rebase.contains("feature/badges") && rebase.contains("main"));
        assert_ne!(merge, rebase);

        assert_eq!(action_note(None, "feature/badges", "main", 3), "");
    }

    #[test]
    fn current_branch_hides_branch_actions() {
        use BranchAction::*;
        let current = overview("main", BranchKind::Local, true);
        let actions = available_actions(&current);
        for hidden in [Checkout, Merge, Rebase, Delete] {
            assert!(
                !actions.contains(&hidden),
                "{hidden:?} shown on the current branch"
            );
        }
        assert_eq!(actions, [Rename, Pin, CopyName, ChangeUpstream, Archive]);

        let other = overview("feature", BranchKind::Local, false);
        assert_eq!(
            available_actions(&other),
            [
                Checkout,
                Merge,
                Rebase,
                Rename,
                Pin,
                CopyName,
                ChangeUpstream,
                Archive,
                Delete
            ]
        );

        let remote = overview("origin/feature", BranchKind::Remote, false);
        assert_eq!(
            available_actions(&remote),
            [Checkout, Merge, Rebase, Pin, CopyName]
        );
    }

    #[test]
    fn remote_groups_are_built_per_remote() {
        let branches = [
            overview("main", BranchKind::Local, true),
            overview("origin/HEAD", BranchKind::Remote, false),
            overview("origin/main", BranchKind::Remote, false),
            overview("upstream/main", BranchKind::Remote, false),
            overview("origin/feature/x", BranchKind::Remote, false),
        ];
        let refs: Vec<&BranchOverview> = branches.iter().collect();
        let groups = remote_groups(&refs);
        let names: Vec<(&str, Vec<&str>)> = groups
            .iter()
            .map(|group| {
                (
                    group.remote,
                    group.branches.iter().map(|b| b.name.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(
            names,
            [
                (
                    "origin",
                    vec!["origin/HEAD", "origin/main", "origin/feature/x"]
                ),
                ("upstream", vec!["upstream/main"]),
            ]
        );
        assert!(remote_groups(&refs[..1]).is_empty());
    }

    #[test]
    fn a_confirmed_action_stops_when_the_repository_changed() {
        let dir = tempfile::tempdir().unwrap();
        let mut init = git2::RepositoryInitOptions::new();
        init.initial_head("main");
        let repo = git2::Repository::init_opts(dir.path(), &init).unwrap();
        let sig = git2::Signature::now("Tester", "tester@example.com").unwrap();
        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let base = repo
            .commit(Some("HEAD"), &sig, &sig, "base", &tree, &[])
            .unwrap();
        let base = repo.find_commit(base).unwrap();
        for name in ["feature", "topic"] {
            repo.branch(name, &base, false).unwrap();
        }
        let path = dir.path().to_str().unwrap();
        let at_base = Some(base.id());
        assert_eq!(
            ensure_unchanged(path, "main", at_base, "topic", true),
            Ok(())
        );

        // `git checkout feature` in a terminal: a band that says "into main" is stale.
        repo.set_head("refs/heads/feature").unwrap();
        assert!(ensure_unchanged(path, "main", at_base, "topic", false).is_err());
        assert_eq!(
            ensure_unchanged(path, "feature", at_base, "topic", true),
            Ok(())
        );

        // topic gets a commit of its own: it is no longer merged into feature.
        let own = repo
            .commit(Some("refs/heads/topic"), &sig, &sig, "own", &tree, &[&base])
            .unwrap();
        assert!(ensure_unchanged(path, "feature", at_base, "topic", true).is_err());
        assert_eq!(
            ensure_unchanged(path, "feature", at_base, "topic", false),
            Ok(())
        );

        // A detached HEAD is named "HEAD" on every commit: moving it is caught by its commit.
        repo.set_head_detached(base.id()).unwrap();
        assert_eq!(
            ensure_unchanged(path, "HEAD", at_base, "topic", false),
            Ok(())
        );
        repo.set_head_detached(own).unwrap();
        assert!(ensure_unchanged(path, "HEAD", at_base, "topic", false).is_err());

        // A commit made on feature in a terminal moves HEAD too.
        repo.set_head("refs/heads/feature").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "next", &tree, &[&base])
            .unwrap();
        assert!(ensure_unchanged(path, "feature", at_base, "topic", false).is_err());
    }

    #[test]
    fn the_tab_opens_on_another_local_branch() {
        let main = overview("main", BranchKind::Local, true);
        let remote = overview("origin/main", BranchKind::Remote, false);
        let list = vec![
            overview("feature", BranchKind::Local, false),
            main.clone(),
            overview("topic", BranchKind::Local, false),
            remote.clone(),
        ];
        let mut panel = BranchPanel::new(Vec::new());
        panel.repo = Some("/repo".into());
        let _ = panel.update(Message::Loaded("/repo".into(), 0, Ok((list.clone(), None))));
        assert_eq!(panel.selected.as_deref(), Some("feature"));

        // Pinned branches come first in the list, so the first of them is picked.
        assert_eq!(initial_selection(&list, &["topic".into()]), Some("topic"));
        // With no other local branch, the current one.
        assert_eq!(initial_selection(&[main, remote], &[]), Some("main"));
        assert_eq!(initial_selection(&[], &[]), None);
    }

    #[test]
    fn the_current_branch_compares_with_its_upstream() {
        let mut main = overview("main", BranchKind::Local, true);
        assert_eq!(RelationCard::new(&main, "main"), None);
        main.upstream = Some("origin/main".into());
        (main.upstream_ahead, main.upstream_behind) = (2, 1);
        let card = |ahead, behind, against: &str| RelationCard {
            ahead,
            behind,
            against: against.into(),
        };
        assert_eq!(
            RelationCard::new(&main, "main"),
            Some(card(2, 1, "origin/main"))
        );

        let mut feature = overview("feature", BranchKind::Local, false);
        (feature.ahead_of_head, feature.behind_head) = (3, 1);
        assert_eq!(
            RelationCard::new(&feature, "main"),
            Some(card(3, 1, "main"))
        );
    }

    #[test]
    fn the_current_branch_lists_its_latest_commits() {
        let dir = tempfile::tempdir().unwrap();
        let mut init = git2::RepositoryInitOptions::new();
        init.initial_head("main");
        let repo = git2::Repository::init_opts(dir.path(), &init).unwrap();
        let sig = git2::Signature::now("Tester", "tester@example.com").unwrap();
        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let mut parents = Vec::new();
        for summary in ["one", "two", "three"] {
            let parent: Vec<&git2::Commit> = parents.last().into_iter().collect();
            let id = repo
                .commit(Some("HEAD"), &sig, &sig, summary, &tree, &parent)
                .unwrap();
            parents.push(repo.find_commit(id).unwrap());
        }
        repo.branch("feature", &parents[1], false).unwrap();
        repo.commit(
            Some("refs/heads/feature"),
            &sig,
            &sig,
            "own",
            &tree,
            &[&parents[1]],
        )
        .unwrap();
        let path = dir.path().to_str().unwrap();
        let summaries = |commits: Vec<CommitInfo>| -> Vec<String> {
            commits.into_iter().map(|commit| commit.summary).collect()
        };

        let latest = read_commits(path, "main", true).unwrap();
        assert_eq!(summaries(latest), ["three", "two", "one"]);
        let own = read_commits(path, "feature", false).unwrap();
        assert_eq!(summaries(own), ["own"]);
    }

    #[test]
    fn stale_means_old_or_no_commit() {
        let now = 1_764_072_000;
        let mut branch = overview("old", BranchKind::Local, false);
        assert!(is_stale(&branch, now));
        branch.last_commit_time = Some(now - 31 * 24 * 60 * 60);
        assert!(is_stale(&branch, now));
        branch.last_commit_time = Some(now - 24 * 60 * 60);
        assert!(!is_stale(&branch, now));
    }
}
