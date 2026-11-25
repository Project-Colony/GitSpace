//! The History tab: the commit list with a filter, a branch picker, ref badges and merge
//! markers, then the selected commit's detail, its changed files and the diff of one file.
//! Git reads run in background tasks; `view` only reads the panel's state.

use std::sync::Arc;

use chrono::{Local, TimeZone};
use colony_ui::theme::{accent_key_to_color, contrast_on, Palette, ACCENT_OVERRIDES};
use colony_ui::{fonts, Typography};
use git2::{Oid, Repository};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, pick_list, row, scrollable, stack, text};
use iced::widget::{text_input, Column, Space};
use iced::{Alignment, Border, Element, Length, Task};

use crate::git::branch::{checkout_branch, create_branch};
use crate::git::diff::{commit_diff_file, commit_diff_summaries, FileDiffSummary};
use crate::git::log::{
    list_local_branches, read_commit_log, CommitFilter, CommitInfo, RefKind, RefLabel,
};
use crate::i18n::{t, tf};
use crate::ui::changes::{field, horizontal_divider, item, question, vertical_divider};
use crate::ui::context::{git_task, RepoContext};
use crate::ui::diff_view::{self, Diff};
use crate::ui::widgets::button as gs_button;
use crate::ui::widgets::chip::{chip, Tone};
use crate::ui::widgets::menu::{self, Entry};
use crate::ui::widgets::toast::Toast;

/// Most commits listed, after every "Load more".
const MAX_COMMITS: usize = 200;
/// Commits added by each "Load more".
const PAGE_SIZE: usize = 50;
/// Width of the commit list.
const LIST_WIDTH: f32 = 360.0;
/// Characters of a short hash, as in `CommitInfo::short_id`.
const SHORT_HASH: usize = 7;
const MORE_GLYPH: &str = "\u{f141}";
const COPY_GLYPH: &str = "\u{f0c5}";

/// One page of commits and the local branches offered by the branch picker.
#[derive(Debug, Clone)]
pub struct LogPage {
    commits: Vec<CommitInfo>,
    branches: Vec<String>,
    /// The number asked for: a full page means more commits may match.
    limit: usize,
}

/// What the detail pane needs besides the commit itself.
#[derive(Debug, Clone)]
pub struct Detail {
    files: Vec<FileDiffSummary>,
    parents: Vec<String>,
}

/// A git action from the commit's "⋯" menu that ended.
#[derive(Debug, Clone)]
pub enum Done {
    /// The short hash now checked out.
    Checkout(String),
    /// The name of the new branch.
    Branch(String),
}

#[derive(Debug, Clone)]
pub enum Message {
    Filter(String),
    /// Applies the filter typed 250 ms ago, unless a later key press replaced it.
    ApplyFilter(u32),
    BranchPicked(String),
    Select(String),
    SelectFile(String),
    LoadMore,
    /// Opens or closes the detail's "⋯" menu.
    CommitMenu(bool),
    CopyHash(String),
    /// Asks to confirm checking out the selected commit, or closes the question.
    ConfirmCheckout(bool),
    Checkout,
    /// Opens the branch name field (`Some`), types in it, or closes it (`None`).
    BranchName(Option<String>),
    CreateBranch,
    /// A read of the log, numbered so that only the latest one is shown.
    Loaded(Arc<str>, u64, Result<LogPage, String>),
    DetailLoaded(Arc<str>, String, Result<Detail, String>),
    /// The patch of `(commit, file)`.
    DiffLoaded(
        Arc<str>,
        (String, String),
        Result<Option<(String, bool)>, String>,
    ),
    /// The key names the failure title.
    ActionDone(Arc<str>, &'static str, Result<Done, String>),
}

/// A commit as the list draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct CommitRow {
    pub short_id: String,
    pub summary: String,
    pub author: String,
    pub initials: String,
    /// A key of `colony_ui::theme::ACCENT_OVERRIDES`.
    pub avatar_accent: &'static str,
    pub when: String,
    pub refs: Vec<RefLabel>,
    pub is_merge: bool,
}

impl CommitRow {
    /// Built when drawn, so the relative date and its language are never stale.
    fn new(commit: &CommitInfo, now: i64) -> Self {
        Self {
            short_id: commit.short_id.clone(),
            summary: commit.summary.clone(),
            author: commit.author.clone(),
            initials: initials(&commit.author),
            avatar_accent: avatar_accent(commit.email.as_deref().unwrap_or(&commit.author)),
            when: relative_time(commit.time.seconds(), now),
            refs: commit.refs.clone(),
            is_merge: commit.parent_count > 1,
        }
    }
}

#[derive(Default)]
pub struct HistoryPanel {
    repo: Option<Arc<str>>,
    filter: String,
    /// The branch shown; `None` follows HEAD.
    branch: Option<String>,
    /// The filter of the last read, reused by "Load more".
    applied: CommitFilter,
    branches: Vec<String>,
    commits: Vec<CommitInfo>,
    error: Option<String>,
    loading: bool,
    has_more: bool,
    /// Number of the last log read; older replies are dropped.
    request: u64,
    filter_version: u32,
    selected: Option<String>,
    detail: Option<Detail>,
    detail_error: Option<String>,
    selected_file: Option<String>,
    diff: Option<Diff>,
    commit_menu: bool,
    confirm_checkout: bool,
    branch_name: Option<String>,
    busy: bool,
}

impl HistoryPanel {
    /// Forgets the previous repository and reads the history of `repo` in the background.
    /// The filter text stays; the branch is cleared when the repository changes.
    pub fn load(&mut self, repo: &RepoContext) -> Task<Message> {
        let same = self.is_current(repo.path());
        *self = Self {
            repo: Some(repo.path_arc()),
            filter: std::mem::take(&mut self.filter),
            branch: if same { self.branch.take() } else { None },
            // Keep request numbers growing so a reply from before the reset is dropped.
            request: self.request,
            ..Self::default()
        };
        self.refresh()
    }

    /// Shows the history of `branch` in `repo`.
    pub fn show_branch(&mut self, branch: String, repo: &RepoContext) -> Task<Message> {
        if !self.is_current(repo.path()) {
            // Only the reset is wanted: a task that is never run never starts its job.
            let _ = self.load(repo);
        }
        self.branch = Some(branch);
        self.refresh()
    }

    /// Handles a message; the toast, if any, is for the application to show.
    pub fn update(&mut self, message: Message) -> (Task<Message>, Option<Toast>) {
        match message {
            Message::Filter(value) => {
                self.filter = value;
                self.filter_version += 1;
                let version = self.filter_version;
                let wait = Task::perform(
                    tokio::time::sleep(std::time::Duration::from_millis(250)),
                    move |_| Message::ApplyFilter(version),
                );
                return (wait, None);
            }
            Message::ApplyFilter(version) => {
                if version == self.filter_version {
                    return (self.refresh(), None);
                }
            }
            Message::BranchPicked(branch) => {
                self.branch = Some(branch);
                return (self.refresh(), None);
            }
            Message::Select(id) => return (self.select(id), None),
            Message::SelectFile(path) => return (self.select_file(path), None),
            Message::LoadMore => {
                // The next page reads the first matching commits again, plus one page.
                if self.has_more && !self.loading {
                    let limit = (self.commits.len() + PAGE_SIZE).min(MAX_COMMITS);
                    return (self.fetch(limit), None);
                }
            }
            Message::CommitMenu(open) => self.commit_menu = open,
            Message::CopyHash(id) => {
                self.commit_menu = false;
                return (iced::clipboard::write(id), None);
            }
            Message::ConfirmCheckout(open) => {
                self.commit_menu = false;
                self.branch_name = None;
                self.confirm_checkout = open;
            }
            Message::Checkout => {
                self.confirm_checkout = false;
                if let Some(id) = self.selected.clone() {
                    let task = self.run("history_checkout_failed", move |repo| {
                        checkout_branch(repo, &id)
                            .map(|()| Done::Checkout(short_hash(&id).to_string()))
                            .map_err(|err| err.to_string())
                    });
                    return (task, None);
                }
            }
            Message::BranchName(name) => {
                self.commit_menu = false;
                self.confirm_checkout = false;
                self.branch_name = name;
            }
            Message::CreateBranch => {
                let name = self.branch_name.as_deref().unwrap_or_default().trim();
                if let (false, Some(id)) = (name.is_empty(), self.selected.clone()) {
                    let name = name.to_string();
                    self.branch_name = None;
                    let task = self.run("history_create_failed", move |repo| {
                        create_branch(repo, &name, Some(&id))
                            .map(|()| Done::Branch(name))
                            .map_err(|err| err.to_string())
                    });
                    return (task, None);
                }
            }
            Message::Loaded(repo, request, result) => {
                if self.is_current(&repo) && request == self.request {
                    return (self.apply_page(result), None);
                }
            }
            Message::DetailLoaded(repo, id, result) => {
                if self.is_current(&repo) && self.selected.as_ref() == Some(&id) {
                    match result {
                        Ok(detail) => {
                            let first = detail.files.first().map(|file| file.path.clone());
                            self.detail = Some(detail);
                            if let Some(path) = first {
                                return (self.select_file(path), None);
                            }
                        }
                        Err(err) => self.detail_error = Some(err),
                    }
                }
            }
            Message::DiffLoaded(repo, key, result) => {
                if self.is_current(&repo)
                    && self.selected.as_ref() == Some(&key.0)
                    && self.selected_file.as_ref() == Some(&key.1)
                {
                    self.diff = Some(Diff::loaded(result));
                }
            }
            Message::ActionDone(repo, failed, result) => {
                if self.is_current(&repo) {
                    self.busy = false;
                    // On success the application reloads every panel, this one included.
                    let toast = match result {
                        Ok(Done::Checkout(hash)) => {
                            Toast::success(tf("history_checked_out", &[("hash", &hash)]))
                        }
                        Ok(Done::Branch(name)) => {
                            Toast::success(tf("history_branch_created", &[("name", &name)]))
                        }
                        Err(err) => Toast::error(t(failed), err),
                    };
                    return (Task::none(), Some(toast));
                }
            }
        }
        (Task::none(), None)
    }

    fn is_current(&self, repo: &str) -> bool {
        self.repo.as_deref() == Some(repo)
    }

    /// Reads the log again from the first page, with the filter and branch shown.
    fn refresh(&mut self) -> Task<Message> {
        self.error = None;
        self.clear_selection();
        let search = self.filter.trim();
        self.applied = CommitFilter {
            branch: self.branch.clone(),
            search: (!search.is_empty()).then(|| search.to_string()),
            ..CommitFilter::default()
        };
        self.fetch(PAGE_SIZE)
    }

    fn clear_selection(&mut self) {
        self.selected = None;
        self.detail = None;
        self.detail_error = None;
        self.selected_file = None;
        self.diff = None;
        self.commit_menu = false;
        self.confirm_checkout = false;
        self.branch_name = None;
    }

    /// Reads the first `limit` commits of the applied filter.
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

    fn apply_page(&mut self, result: Result<LogPage, String>) -> Task<Message> {
        self.loading = false;
        match result {
            Ok(page) => {
                self.has_more = page.commits.len() == page.limit && page.limit < MAX_COMMITS;
                self.commits = page.commits;
                self.branches = page.branches;
                // "Load more" keeps the selection; a new read selects the newest commit.
                match (&self.selected, self.commits.first()) {
                    (None, Some(first)) => {
                        let id = first.id.clone();
                        self.select(id)
                    }
                    _ => Task::none(),
                }
            }
            Err(err) => {
                self.commits.clear();
                self.has_more = false;
                self.error = Some(err);
                Task::none()
            }
        }
    }

    fn select(&mut self, id: String) -> Task<Message> {
        self.clear_selection();
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        self.selected = Some(id.clone());
        let job_id = id.clone();
        git_task(
            repo,
            move |repo| read_detail(repo, &job_id),
            move |repo, result| Message::DetailLoaded(repo, id.clone(), result),
        )
    }

    fn select_file(&mut self, path: String) -> Task<Message> {
        let (Some(repo), Some(id)) = (self.repo.clone(), self.selected.clone()) else {
            return Task::none();
        };
        self.selected_file = Some(path.clone());
        self.diff = Some(Diff::Loading);
        let key = (id.clone(), path.clone());
        git_task(
            repo,
            move |repo| {
                commit_diff_file(repo, &id, &path)
                    .map(|diff| diff.map(|diff| (diff.patch, diff.truncated)))
                    .map_err(|err| err.to_string())
            },
            move |repo, result| Message::DiffLoaded(repo, key.clone(), result),
        )
    }

    /// Runs a git action unless one is running.
    fn run(
        &mut self,
        failed: &'static str,
        job: impl FnOnce(&str) -> Result<Done, String> + Send + 'static,
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
        let list = column![self.toolbar(), self.commit_list()]
            .width(Length::Fixed(LIST_WIDTH))
            .height(Length::Fill);
        row![list, vertical_divider(), self.detail_pane()]
            .height(Length::Fill)
            .into()
    }

    /// The filter field and the branch picker.
    fn toolbar(&self) -> Element<'_, Message> {
        let typo = Typography::current();
        let filter = field(
            text_input(t("history_filter_placeholder"), &self.filter).on_input(Message::Filter),
            None,
        );
        let branch = pick_list(
            self.branches.as_slice(),
            self.branch.clone(),
            Message::BranchPicked,
        )
        .placeholder(t("history_current_branch"))
        .text_size(typo.sz(12))
        .font(typo.regular);
        container(row![filter, branch].spacing(6).align_y(Alignment::Center))
            .padding([10, 10])
            .into()
    }

    fn commit_list(&self) -> Element<'_, Message> {
        if self.commits.is_empty() {
            let label = match &self.error {
                Some(err) => tf("history_load_failed", &[("error", err)]),
                None if self.loading => t("changes_loading").to_string(),
                None if self.applied.search.is_some() => t("history_no_match").to_string(),
                None => t("history_empty").to_string(),
            };
            return note(label);
        }
        let now = chrono::Utc::now().timestamp();
        let mut rows = Column::new().padding([0, 6]);
        for commit in &self.commits {
            let selected = self.selected.as_deref() == Some(commit.id.as_str());
            rows = rows.push(commit_row(
                CommitRow::new(commit, now),
                commit.id.clone(),
                selected,
            ));
        }
        if self.has_more {
            rows = rows.push(
                container(gs_button::ghost(
                    t("history_load_more"),
                    (!self.loading).then_some(Message::LoadMore),
                ))
                .center_x(Length::Fill)
                .padding([8, 0]),
            );
        }
        scrollable(rows).height(Length::Fill).into()
    }

    fn detail_pane(&self) -> Element<'_, Message> {
        let selected = self
            .selected
            .as_ref()
            .and_then(|id| self.commits.iter().find(|commit| &commit.id == id));
        let Some(commit) = selected else {
            return note(t("history_select_commit").to_string());
        };
        let mut files = Column::new().spacing(1);
        if let Some(err) = &self.detail_error {
            files = files.push(note_line(err.clone()));
        }
        for file in self.detail.iter().flat_map(|detail| &detail.files) {
            files = files.push(self.file_row(file));
        }
        let files = container(scrollable(files.padding([6, 10]))).max_height(180.0);
        column![
            self.detail_header(commit),
            horizontal_divider(),
            files,
            horizontal_divider(),
            diff_view::pane(self.diff.as_ref(), t("history_select_file")),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// The summary with the "⋯" menu, the body, the author, date, hash and parents, then a
    /// pending checkout question or branch name field.
    fn detail_header<'a>(&'a self, commit: &'a CommitInfo) -> Element<'a, Message> {
        let typo = Typography::current();
        let entries = vec![
            item(
                t("history_copy_hash"),
                Message::CopyHash(commit.id.clone()),
                false,
            ),
            Entry::Separator,
            item(t("history_checkout"), Message::ConfirmCheckout(true), false),
            item(
                t("history_create_branch"),
                Message::BranchName(Some(String::new())),
                false,
            ),
        ];
        let more = menu::popup(
            gs_button::icon(
                MORE_GLYPH,
                Some(Message::CommitMenu(!self.commit_menu)),
                t("history_commit_actions"),
            ),
            self.commit_menu,
            entries,
            Message::CommitMenu(false),
        );
        let title = row![
            text(commit.summary.as_str())
                .size(typo.sz(15))
                .font(typo.bold)
                .color(Palette::TEXT_PRIMARY())
                .width(Length::Fill),
            more,
        ]
        .spacing(10)
        .align_y(Alignment::Start);

        let small = |label: String| {
            text(label)
                .size(typo.sz(11))
                .font(typo.regular)
                .color(Palette::TEXT_DIM())
        };
        let date = Local
            .timestamp_opt(commit.time.seconds(), 0)
            .single()
            .map(|date| date.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();
        let hash = button(
            row![
                text(commit.short_id.as_str())
                    .size(typo.sz(11))
                    .font(typo.regular)
                    .color(Palette::TEXT_SECONDARY()),
                text(COPY_GLYPH)
                    .size(typo.sz(10))
                    .font(fonts::glyph_font())
                    .color(Palette::TEXT_DIMMER()),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([2, 7])
        .on_press(Message::CopyHash(commit.id.clone()))
        .style(|_, status| button::Style {
            background: Some(
                if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                    Palette::BG_SELECTED()
                } else {
                    Palette::BG_CARD_HOVER()
                }
                .into(),
            ),
            border: Border::default().rounded(4),
            ..Default::default()
        });
        let mut meta = row![
            avatar(
                &initials(&commit.author),
                avatar_accent(commit.email.as_deref().unwrap_or(&commit.author)),
                18.0,
            ),
            small(commit.author.clone()),
            small(date),
            gs_button::with_tip(hash, t("history_copy_hash")),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        if let Some(detail) = &self.detail {
            let parents: Vec<&str> = detail.parents.iter().map(|id| short_hash(id)).collect();
            match parents.as_slice() {
                [] => {}
                [one] => meta = meta.push(small(tf("history_parent", &[("hash", one)]))),
                many => {
                    meta = meta.push(small(tf(
                        "history_parents",
                        &[("hashes", &many.join(", "))],
                    )))
                }
            }
        }

        let mut header = column![title].spacing(8).padding([16, 18]);
        let body = commit_body(commit);
        if !body.is_empty() {
            header = header.push(
                text(body)
                    .size(typo.sz(12))
                    .font(typo.regular)
                    .color(Palette::TEXT_DIM()),
            );
        }
        header = header.push(meta);

        let mut bands = column![header];
        let prompt = |label: String| -> Element<'a, Message> {
            text(label)
                .size(typo.sz(12))
                .font(typo.regular)
                .color(Palette::TEXT_SECONDARY())
                .into()
        };
        let hash = commit.short_id.as_str();
        if self.confirm_checkout {
            bands = bands.push(question(
                vec![prompt(tf("history_checkout_confirm", &[("hash", hash)]))],
                gs_button::primary(
                    t("history_checkout_button"),
                    (!self.busy).then_some(Message::Checkout),
                ),
                Message::ConfirmCheckout(false),
            ));
        }
        if let Some(name) = &self.branch_name {
            let ready = !self.busy && !name.trim().is_empty();
            bands = bands.push(question(
                vec![
                    prompt(tf("history_branch_prompt", &[("hash", hash)])),
                    container(field(
                        text_input(t("history_branch_name"), name)
                            .on_input(|name| Message::BranchName(Some(name)))
                            .on_submit_maybe(ready.then_some(Message::CreateBranch)),
                        None,
                    ))
                    .width(Length::Fixed(220.0))
                    .into(),
                ],
                gs_button::primary(
                    t("history_create_button"),
                    ready.then_some(Message::CreateBranch),
                ),
                Message::BranchName(None),
            ));
        }
        bands.into()
    }

    /// A changed file: its name and folder, +N −N and the five-cell bar.
    fn file_row<'a>(&'a self, file: &'a FileDiffSummary) -> Element<'a, Message> {
        let typo = Typography::current();
        let selected = self.selected_file.as_deref() == Some(file.path.as_str());
        let (folder, name) = file
            .path
            .rsplit_once('/')
            .unwrap_or(("", file.path.as_str()));
        let mut content = row![
            text(name)
                .size(typo.sz(12))
                .font(typo.regular)
                .color(Palette::TEXT_PRIMARY()),
            text(folder)
                .size(typo.sz(10))
                .font(typo.regular)
                .color(Palette::TEXT_DIMMEST())
                .width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if file.additions > 0 {
            content = content.push(
                text(format!("+{}", file.additions))
                    .size(typo.sz(10))
                    .font(typo.regular)
                    .color(Palette::SUCCESS()),
            );
        }
        if file.deletions > 0 {
            content = content.push(
                text(format!("−{}", file.deletions))
                    .size(typo.sz(10))
                    .font(typo.regular)
                    .color(Palette::ERROR()),
            );
        }
        if !file.is_binary {
            content = content.push(change_bar(file.additions, file.deletions));
        }
        button(content)
            .on_press(Message::SelectFile(file.path.clone()))
            .padding([5, 8])
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
}

/// A list row: the lane dot, the avatar, the summary with its ref badges over
/// "author · date", and the short hash.
fn commit_row<'a>(commit: CommitRow, id: String, selected: bool) -> Element<'a, Message> {
    let typo = Typography::current();
    let mut line = row![container(
        text(commit.summary)
            .size(typo.sz(12))
            .font(typo.regular)
            .color(Palette::TEXT_PRIMARY())
            .wrapping(Wrapping::None),
    )
    .width(Length::Fill)
    .clip(true)]
    .spacing(6)
    .align_y(Alignment::Center);
    for label in &commit.refs {
        let tone = match label.kind {
            RefKind::Local | RefKind::Head => Tone::Accent,
            RefKind::Remote => Tone::Remote,
            RefKind::Tag => Tone::Tag,
        };
        line = line.push(chip(&label.name, tone));
    }
    let meta = text(format!("{} · {}", commit.author, commit.when))
        .size(typo.sz(11))
        .font(typo.regular)
        .color(Palette::TEXT_DIMMEST())
        .wrapping(Wrapping::None);
    let content = row![
        lane(commit.is_merge),
        avatar(&commit.initials, commit.avatar_accent, 22.0),
        column![line, meta]
            .spacing(2)
            .width(Length::Fill)
            .padding([7, 0]),
        text(commit.short_id)
            .size(typo.sz(11))
            .font(typo.regular)
            .color(Palette::TEXT_DIMMER()),
    ]
    .spacing(10)
    .height(Length::Fill)
    .align_y(Alignment::Center);
    // No vertical padding: the lane runs the full height of the row.
    button(content)
        .on_press(Message::Select(id))
        .padding(iced::Padding {
            top: 0.0,
            right: 10.0,
            bottom: 0.0,
            left: 6.0,
        })
        .width(Length::Fill)
        .height(Length::Fixed(row_height()))
        .style(move |_, status| button::Style {
            background: (selected
                || matches!(status, button::Status::Hovered | button::Status::Pressed))
            .then(|| Palette::BG_CARD_HOVER().into()),
            border: Border::default().rounded(6),
            ..Default::default()
        })
        .into()
}

/// A row's height at the current text size: its padding, the summary line (as tall as a
/// badge) and the meta line. Fixed so the lane line can run from row to row.
fn row_height() -> f32 {
    let typo = Typography::current();
    let line = |size: f32| size * 1.3;
    let badge = line(typo.sz(11)) + 6.0;
    14.0 + line(typo.sz(12)).max(badge) + 2.0 + line(typo.sz(11)) + 2.0
}

/// The vertical line through the rows with this commit's dot: filled for a commit,
/// a hollow ring for a merge.
fn lane<'a>(is_merge: bool) -> Element<'a, Message> {
    let line = container(Space::new().width(2).height(Length::Fill)).style(|_| container::Style {
        background: Some(Palette::BG_SELECTED().into()),
        ..Default::default()
    });
    let dot = container(Space::new().width(9).height(9)).style(move |_| container::Style {
        background: Some(
            if is_merge {
                Palette::BG_PRIMARY()
            } else {
                Palette::ACCENT()
            }
            .into(),
        ),
        border: Border::default()
            .rounded(5)
            .width(if is_merge { 2 } else { 0 })
            .color(Tone::Remote.color()),
        ..Default::default()
    });
    stack![
        container(line)
            .center_x(Length::Fixed(16.0))
            .height(Length::Fill),
        container(dot)
            .center_x(Length::Fixed(16.0))
            .center_y(Length::Fill),
    ]
    .width(Length::Fixed(16.0))
    .height(Length::Fill)
    .into()
}

/// The author's initials in a circle of their accent colour.
fn avatar<'a>(initials: &str, accent: &str, size: f32) -> Element<'a, Message> {
    let typo = Typography::current();
    let color = accent_key_to_color(accent).unwrap_or_else(Palette::ACCENT);
    container(
        text(initials.to_string())
            .size(typo.sz(if size < 20.0 { 9 } else { 10 }))
            .font(typo.bold)
            .color(contrast_on(color)),
    )
    .center_x(Length::Fixed(size))
    .center_y(Length::Fixed(size))
    .style(move |_| container::Style {
        background: Some(color.into()),
        border: Border::default().rounded(size / 2.0),
        ..Default::default()
    })
    .into()
}

/// Five cells: green for added lines, red for removed ones, one cell per changed line up
/// to five, then shared out in proportion.
fn change_bar<'a>(additions: usize, deletions: usize) -> Element<'a, Message> {
    let (added, removed) = bar_cells(additions, deletions);
    let mut cells = row![].spacing(1);
    for index in 0..5 {
        let color = if index < added {
            Palette::SUCCESS()
        } else if index < added + removed {
            Palette::ERROR()
        } else {
            Palette::BG_SELECTED()
        };
        cells = cells.push(container(Space::new().width(6).height(8)).style(move |_| {
            container::Style {
                background: Some(color.into()),
                border: Border::default().rounded(1),
                ..Default::default()
            }
        }));
    }
    cells.into()
}

/// The green and red cells of the five-cell bar.
fn bar_cells(additions: usize, deletions: usize) -> (usize, usize) {
    let total = additions + deletions;
    if total == 0 {
        return (0, 0);
    }
    let filled = total.min(5);
    let added = (filled * additions + total / 2) / total;
    // A file with removals shows at least one red cell.
    let added = if deletions > 0 {
        added.min(filled - 1)
    } else {
        added
    };
    (added, filled - added)
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

/// The message without its summary line.
fn commit_body(commit: &CommitInfo) -> &str {
    let message = commit.message.trim();
    message
        .strip_prefix(commit.summary.trim())
        .unwrap_or(message)
        .trim_start()
}

fn short_hash(id: &str) -> &str {
    id.get(..SHORT_HASH).unwrap_or(id)
}

/// The first letters of the first two words of `author`, upper case. Words are split on
/// anything that is not a letter or digit, so "release-please" gives "RP".
fn initials(author: &str) -> String {
    author
        .split(|c: char| !c.is_alphanumeric())
        .filter_map(|word| word.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

/// The accent key of an author's avatar, the same on every start and machine.
fn avatar_accent(email_or_name: &str) -> &'static str {
    // A fixed string hash: std's hashers may change between Rust releases.
    let hash = email_or_name
        .to_lowercase()
        .bytes()
        .fold(0u32, |hash, byte| {
            hash.wrapping_mul(31).wrapping_add(u32::from(byte))
        });
    ACCENT_OVERRIDES[hash as usize % ACCENT_OVERRIDES.len()].key
}

/// "just now", "5 min ago", "3 h ago", "yesterday", "4 days ago", then the date.
fn relative_time(then: i64, now: i64) -> String {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    const TWO_DAYS: i64 = 2 * DAY;
    const WEEK: i64 = 7 * DAY;
    let elapsed = now - then;
    let count = |unit: i64| (elapsed / unit).to_string();
    match elapsed {
        // A commit from a clock ahead of this one counts as new.
        i64::MIN..MINUTE => t("time_just_now").to_string(),
        MINUTE..HOUR => tf("time_minutes_ago", &[("count", &count(MINUTE))]),
        HOUR..DAY => tf("time_hours_ago", &[("count", &count(HOUR))]),
        DAY..TWO_DAYS => t("time_yesterday").to_string(),
        TWO_DAYS..WEEK => tf("time_days_ago", &[("count", &count(DAY))]),
        _ => Local
            .timestamp_opt(then, 0)
            .single()
            .map(|date| date.format("%Y-%m-%d").to_string())
            .unwrap_or_default(),
    }
}

/// Reads the first `limit` commits matching `filter` and the local branch names.
fn read_page(repo: &str, filter: &CommitFilter, limit: usize) -> Result<LogPage, String> {
    let commits = read_commit_log(repo, filter, limit, false).map_err(|err| err.to_string())?;
    let branches = list_local_branches(repo).unwrap_or_default();
    Ok(LogPage {
        commits,
        branches,
        limit,
    })
}

/// The changed files and the parent hashes of commit `id`.
fn read_detail(repo_path: &str, id: &str) -> Result<Detail, String> {
    let files = commit_diff_summaries(repo_path, id).map_err(|err| err.to_string())?;
    let parents = Oid::from_str(id)
        .and_then(|oid| {
            let repo = Repository::open(repo_path)?;
            let commit = repo.find_commit(oid)?;
            Ok(commit.parent_ids().map(|id| id.to_string()).collect())
        })
        .map_err(|err| err.to_string())?;
    Ok(Detail { files, parents })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_take_two_words() {
        assert_eq!(initials("Ada Lovelace Byron"), "AL");
        assert_eq!(initials("release-please[bot]"), "RP");
        assert_eq!(initials("élise durand"), "ÉD");
        assert_eq!(initials("Torvalds"), "T");
        assert_eq!(initials("  "), "");
    }

    #[test]
    fn avatar_accent_is_stable_per_author() {
        let keys: Vec<&str> = ACCENT_OVERRIDES.iter().map(|accent| accent.key).collect();
        let first = avatar_accent("ada@example.com");
        assert!(keys.contains(&first));
        assert_eq!(avatar_accent("ada@example.com"), first);
        // A fixed hash, not a seeded one: these values hold on every machine.
        let spread: std::collections::HashSet<_> = (0..40)
            .map(|n| avatar_accent(&format!("author{n}@example.com")))
            .collect();
        assert!(spread.len() > 3, "authors spread over the accents");
    }

    #[test]
    fn relative_time_buckets() {
        let now = 1_764_072_000; // 2025-11-25 12:00 UTC
        let minute = 60;
        let hour = 60 * minute;
        let day = 24 * hour;
        assert_eq!(relative_time(now - 20, now), t("time_just_now"));
        assert_eq!(
            relative_time(now - 5 * minute, now),
            tf("time_minutes_ago", &[("count", "5")])
        );
        assert_eq!(
            relative_time(now - 3 * hour, now),
            tf("time_hours_ago", &[("count", "3")])
        );
        assert_eq!(relative_time(now - 30 * hour, now), t("time_yesterday"));
        assert_eq!(
            relative_time(now - 4 * day, now),
            tf("time_days_ago", &[("count", "4")])
        );
        // Noon UTC is the same calendar day in every time zone within ±11 h.
        assert_eq!(relative_time(now - 10 * day, now), "2025-11-15");
        // A commit dated in the future, from a skewed clock.
        assert_eq!(relative_time(now + hour, now), t("time_just_now"));
    }

    #[test]
    fn bar_cells_scale_to_five() {
        assert_eq!(bar_cells(0, 0), (0, 0));
        assert_eq!(bar_cells(1, 0), (1, 0));
        assert_eq!(bar_cells(5, 1), (4, 1));
        assert_eq!(bar_cells(100, 0), (5, 0));
        assert_eq!(bar_cells(100, 1), (4, 1));
        assert_eq!(bar_cells(1, 1), (1, 1));
    }

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

        let head = parent.unwrap();
        let detail = read_detail(path, &head.id().to_string()).unwrap();
        assert_eq!(detail.parents, [head.parent_id(0).unwrap().to_string()]);
    }
}
