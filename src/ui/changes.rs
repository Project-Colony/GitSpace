//! The Changes tab: staged and unstaged files with their line counts, the diff of the
//! selected file, and the commit box.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use colony_ui::theme::{contrast_on, Palette};
use colony_ui::widgets::icons;
use colony_ui::{fonts, Typography};
use git2::{ObjectType, Repository};
use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input};
use iced::widget::{Column, Space};
use iced::{Alignment, Border, Color, Element, Length, Task};

use crate::git::branch::{list_branches, restore_file_from_branch};
use crate::git::commit::commit_index;
use crate::git::diff::diff_file;
use crate::git::status::{discard_file, read_changes, read_repo_status, ChangeKind, ChangedFile};
use crate::i18n::{t, tf};
use crate::ui::context::{git_task, RepoContext};
use crate::ui::diff_view::{self, Diff};
use crate::ui::widgets::button as gs_button;
use crate::ui::widgets::menu::{self, Entry, MenuItem};
use crate::ui::widgets::toast::Toast;

const MORE_GLYPH: &str = "\u{f141}";
/// The summary length past which the counter turns to the warning colour.
const SUMMARY_SOFT_LIMIT: usize = 72;
/// Width of the file list.
const LIST_WIDTH: f32 = 270.0;

/// Label key and text key of each commit message template.
const TEMPLATES: [(&str, &str); 3] = [
    ("changes_template_wip", "changes_template_wip_text"),
    ("changes_template_feature", "changes_template_feature_text"),
    ("changes_template_fix", "changes_template_fix_text"),
];

/// What the repository looks like to the Changes tab, read off the UI thread.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    staged: Vec<ChangedFile>,
    unstaged: Vec<ChangedFile>,
    branch: Option<String>,
    author: Option<String>,
    signoff_line: Option<String>,
    /// Local then remote branches, for "Restore from branch…".
    branches: Vec<String>,
}

/// A step of "Restore from branch…".
#[derive(Debug, Clone)]
pub enum Restore {
    /// Asks which branch to restore this file from.
    Open(String),
    Branch(String),
    Confirm,
    Cancel,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Arc<str>, Result<Snapshot, String>),
    /// Shows the diff of a file, staged or not.
    Select(String, bool),
    /// The patch of the file `(path, staged)`.
    DiffLoaded(
        Arc<str>,
        (String, bool),
        Result<Option<(String, bool)>, String>,
    ),
    /// Stages an unstaged file, or unstages a staged one.
    Toggle(String, bool),
    StageAll,
    UnstageAll,
    /// Asks to confirm discarding the changes of a file, or closes the question.
    ConfirmDiscard(Option<String>),
    Discard(String),
    Summary(String),
    Description(String),
    TemplateSelected(usize),
    ToggleSignoff,
    /// Opens or closes the commit box's "⋯" menu.
    CommitMenu(bool),
    Commit,
    /// The new commit's short hash, or the error.
    Committed(Arc<str>, Result<String, String>),
    /// A stage, unstage, discard or restore ended; the key names the failure title.
    ActionDone(Arc<str>, &'static str, Result<(), String>),
    /// Opens the "⋯" menu of the diff header for a file, or closes it.
    FileMenu(Option<String>),
    RestoreFromBranch(Restore),
    OpenFile(String),
    CopyPath(String),
}

#[derive(Default)]
pub struct ChangesPanel {
    repo: Option<Arc<str>>,
    snapshot: Option<Snapshot>,
    /// Files listed in either group, each counted once.
    count: usize,
    selected: Option<(String, bool)>,
    diff: Option<Diff>,
    busy: bool,
    summary: String,
    description: String,
    signoff: bool,
    file_menu: Option<String>,
    commit_menu: bool,
    confirm_discard: Option<String>,
    /// The file and branch of a restore waiting for confirmation.
    restore: Option<(String, Option<String>)>,
}

impl ChangesPanel {
    /// Reads the changes of `repo` off the UI thread. Reloading the same repository keeps
    /// the commit drafts and the selected file.
    pub fn load(&mut self, repo: &RepoContext) -> Task<Message> {
        if self.repo.as_deref() != Some(repo.path()) {
            *self = Self {
                repo: Some(repo.path_arc()),
                ..Self::default()
            };
        }
        self.refresh()
    }

    /// The number of changed files, shown on the Changes tab.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Handles a message; the toast, if any, is for the application to show.
    pub fn update(&mut self, message: Message) -> (Task<Message>, Option<Toast>) {
        match message {
            Message::Loaded(repo, result) => {
                if !self.is_current(&repo) {
                    return (Task::none(), None);
                }
                match result {
                    Ok(snapshot) => (self.apply_snapshot(snapshot), None),
                    Err(err) => (
                        Task::none(),
                        Some(Toast::error(t("changes_load_failed"), err)),
                    ),
                }
            }
            Message::Select(path, staged) => {
                self.file_menu = None;
                self.selected = Some((path, staged));
                self.diff = Some(Diff::Loading);
                (self.load_diff(), None)
            }
            Message::DiffLoaded(repo, key, result) => {
                if self.is_current(&repo) && self.selected.as_ref() == Some(&key) {
                    self.diff = Some(Diff::loaded(result));
                }
                (Task::none(), None)
            }
            Message::Toggle(path, staged) => {
                let paths = self.with_old_paths(staged, |file| file.path == path);
                (self.stage(paths, staged), None)
            }
            Message::StageAll => {
                let paths = self.with_old_paths(false, |_| true);
                (self.stage(paths, false), None)
            }
            Message::UnstageAll => {
                let paths = self.with_old_paths(true, |_| true);
                (self.stage(paths, true), None)
            }
            Message::ConfirmDiscard(path) => {
                self.file_menu = None;
                self.restore = None;
                self.confirm_discard = path;
                (Task::none(), None)
            }
            Message::Discard(path) => {
                self.confirm_discard = None;
                let task = self.run("changes_discard_failed", move |repo| {
                    discard_file(repo, &path).map_err(|err| err.to_string())
                });
                (task, None)
            }
            Message::Summary(summary) => {
                self.summary = summary;
                (Task::none(), None)
            }
            Message::Description(description) => {
                self.description = description;
                (Task::none(), None)
            }
            Message::TemplateSelected(index) => {
                self.commit_menu = false;
                if let Some((_, text_key)) = TEMPLATES.get(index) {
                    let (summary, description) =
                        t(text_key).split_once("\n\n").unwrap_or((t(text_key), ""));
                    self.summary = summary.to_string();
                    self.description = description.to_string();
                }
                (Task::none(), None)
            }
            Message::ToggleSignoff => {
                self.commit_menu = false;
                self.signoff = !self.signoff && self.signoff_line().is_some();
                (Task::none(), None)
            }
            Message::CommitMenu(open) => {
                self.commit_menu = open;
                (Task::none(), None)
            }
            Message::Commit => {
                if !self.can_commit() {
                    return (Task::none(), None);
                }
                let message = commit_message(
                    &self.summary,
                    &self.description,
                    self.signoff.then(|| self.signoff_line()).flatten(),
                );
                let Some(repo) = self.repo.clone() else {
                    return (Task::none(), None);
                };
                self.busy = true;
                let task = git_task(
                    repo,
                    move |repo| {
                        commit_index(repo, &message)
                            .map(|oid| oid.to_string()[..7].to_string())
                            .map_err(|err| err.to_string())
                    },
                    Message::Committed,
                );
                (task, None)
            }
            Message::Committed(repo, result) => {
                if !self.is_current(&repo) {
                    return (Task::none(), None);
                }
                self.busy = false;
                let toast = match result {
                    Ok(hash) => {
                        self.summary.clear();
                        self.description.clear();
                        Toast::success(tf("changes_committed", &[("hash", &hash)]))
                    }
                    Err(err) => Toast::error(t("changes_commit_failed"), err),
                };
                (self.refresh(), Some(toast))
            }
            Message::ActionDone(repo, failed, result) => {
                if !self.is_current(&repo) {
                    return (Task::none(), None);
                }
                self.busy = false;
                let toast = result.err().map(|err| Toast::error(t(failed), err));
                (self.refresh(), toast)
            }
            Message::FileMenu(path) => {
                self.file_menu = path;
                (Task::none(), None)
            }
            Message::RestoreFromBranch(step) => self.restore_step(step),
            Message::OpenFile(path) => {
                self.file_menu = None;
                let toast = self.repo.as_deref().and_then(|repo| {
                    open::that_detached(Path::new(repo).join(&path))
                        .err()
                        .map(|err| Toast::error(t("changes_open_file_failed"), err.to_string()))
                });
                (Task::none(), toast)
            }
            Message::CopyPath(path) => {
                self.file_menu = None;
                let Some(repo) = self.repo.as_deref() else {
                    return (Task::none(), None);
                };
                let full = Path::new(repo).join(path).to_string_lossy().into_owned();
                (iced::clipboard::write(full), None)
            }
        }
    }

    fn restore_step(&mut self, step: Restore) -> (Task<Message>, Option<Toast>) {
        match step {
            Restore::Open(path) => {
                self.file_menu = None;
                self.confirm_discard = None;
                let branch = self.snapshot.as_ref().and_then(|s| s.branch.clone());
                self.restore = Some((path, branch));
            }
            Restore::Branch(branch) => {
                if let Some((_, chosen)) = &mut self.restore {
                    *chosen = Some(branch);
                }
            }
            Restore::Cancel => self.restore = None,
            Restore::Confirm => {
                if let Some((path, Some(branch))) = self.restore.take() {
                    let task = self.run("changes_restore_failed", move |repo| {
                        restore_file_from_branch(repo, &branch, &path)
                    });
                    return (task, None);
                }
            }
        }
        (Task::none(), None)
    }

    fn is_current(&self, repo: &str) -> bool {
        self.repo.as_deref() == Some(repo)
    }

    fn signoff_line(&self) -> Option<&str> {
        self.snapshot.as_ref()?.signoff_line.as_deref()
    }

    fn staged(&self) -> &[ChangedFile] {
        self.snapshot.as_ref().map_or(&[], |s| &s.staged)
    }

    fn unstaged(&self) -> &[ChangedFile] {
        self.snapshot.as_ref().map_or(&[], |s| &s.unstaged)
    }

    /// A commit needs a summary and at least one staged file.
    fn can_commit(&self) -> bool {
        !self.busy && !self.summary.trim().is_empty() && !self.staged().is_empty()
    }

    /// The paths of the matching files of one group, with the old path of a renamed one so
    /// unstaging it also puts back the deletion.
    fn with_old_paths(&self, staged: bool, keep: impl Fn(&ChangedFile) -> bool) -> Vec<String> {
        let files = if staged {
            self.staged()
        } else {
            self.unstaged()
        };
        files
            .iter()
            .filter(|file| keep(file))
            .flat_map(|file| std::iter::once(file.path.clone()).chain(file.old_path.clone()))
            .collect()
    }

    /// Stages `paths`, or unstages them when `staged` is true.
    fn stage(&mut self, paths: Vec<String>, staged: bool) -> Task<Message> {
        if paths.is_empty() {
            return Task::none();
        }
        if staged {
            self.run("changes_unstage_failed", move |repo| {
                unstage_paths(repo, &paths).map_err(|err| err.to_string())
            })
        } else {
            self.run("changes_stage_failed", move |repo| {
                stage_paths(repo, &paths).map_err(|err| err.to_string())
            })
        }
    }

    /// Runs a git action unless one is running; the changes are read again when it ends.
    fn run(
        &mut self,
        failed: &'static str,
        job: impl FnOnce(&str) -> Result<(), String> + Send + 'static,
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

    fn refresh(&self) -> Task<Message> {
        match self.repo.clone() {
            Some(repo) => git_task(repo, read_snapshot, Message::Loaded),
            None => Task::none(),
        }
    }

    fn load_diff(&mut self) -> Task<Message> {
        let (Some(repo), Some(key)) = (self.repo.clone(), self.selected.clone()) else {
            return Task::none();
        };
        let (path, staged) = key.clone();
        git_task(
            repo,
            move |repo| {
                diff_file(repo, &path, staged)
                    .map(|diff| diff.map(|diff| (diff.patch, diff.truncated)))
                    .map_err(|err| err.to_string())
            },
            move |repo, result| Message::DiffLoaded(repo, key.clone(), result),
        )
    }

    /// Shows `snapshot`. The selected file stays selected, in the other group if it moved
    /// there; with no selection left, the first file is selected.
    fn apply_snapshot(&mut self, snapshot: Snapshot) -> Task<Message> {
        if snapshot.signoff_line.is_none() {
            self.signoff = false;
        }
        self.count = snapshot
            .staged
            .iter()
            .chain(&snapshot.unstaged)
            .map(|file| file.path.as_str())
            .collect::<HashSet<_>>()
            .len();
        let listed = |path: &str, staged: bool| {
            let files = if staged {
                &snapshot.staged
            } else {
                &snapshot.unstaged
            };
            files.iter().any(|file| file.path == path)
        };
        let previous = self.selected.take();
        let selected = match previous.clone() {
            Some((path, staged)) if listed(&path, staged) => Some((path, staged)),
            Some((path, staged)) if listed(&path, !staged) => Some((path, !staged)),
            _ => None,
        }
        .or_else(|| {
            let first = |files: &[ChangedFile], staged| {
                files.first().map(|file| (file.path.clone(), staged))
            };
            first(&snapshot.unstaged, false).or_else(|| first(&snapshot.staged, true))
        });
        self.snapshot = Some(snapshot);
        // Reloading the shown file keeps its diff and scroll offset until the new one arrives.
        if selected != previous {
            self.diff = Some(Diff::Loading);
        }
        self.selected = selected;
        if self.selected.is_some() {
            self.load_diff()
        } else {
            self.diff = None;
            Task::none()
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let Some(snapshot) = &self.snapshot else {
            return Space::new().into();
        };
        if snapshot.staged.is_empty() && snapshot.unstaged.is_empty() {
            return self.view_nothing(snapshot);
        }
        let list = scrollable(
            column![
                self.group(true, &snapshot.staged),
                self.group(false, &snapshot.unstaged),
            ]
            .padding([10, 8]),
        )
        .height(Length::Fill);
        let list = container(list)
            .width(Length::Fixed(LIST_WIDTH))
            .height(Length::Fill);

        let right = column![
            self.diff_header(),
            diff_view::pane(self.diff.as_ref(), t("changes_select_file")),
            self.commit_box(snapshot)
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        row![list, vertical_divider(), right]
            .height(Length::Fill)
            .into()
    }

    fn view_nothing(&self, snapshot: &Snapshot) -> Element<'_, Message> {
        let typo = Typography::current();
        let branch = snapshot.branch.as_deref().unwrap_or("HEAD");
        container(
            column![
                text(t("changes_nothing_title"))
                    .size(typo.sz(15))
                    .font(typo.bold)
                    .color(Palette::TEXT_PRIMARY()),
                text(tf("changes_nothing_detail", &[("branch", branch)]))
                    .size(typo.sz(12))
                    .font(typo.regular)
                    .color(Palette::TEXT_DIMMER()),
            ]
            .spacing(6)
            .align_x(Alignment::Center),
        )
        .center(Length::Fill)
        .into()
    }

    /// "STAGED · n" or "UNSTAGED · n" with its "Unstage all" or "Stage all" link, then the files.
    fn group<'a>(&'a self, staged: bool, files: &'a [ChangedFile]) -> Element<'a, Message> {
        let typo = Typography::current();
        let count = files.len().to_string();
        let (title, link, all) = if staged {
            (
                tf("changes_staged", &[("count", &count)]),
                t("changes_unstage_all"),
                Message::UnstageAll,
            )
        } else {
            (
                tf("changes_unstaged", &[("count", &count)]),
                t("changes_stage_all"),
                Message::StageAll,
            )
        };
        let link = button(
            text(link)
                .size(typo.sz(11))
                .font(typo.regular)
                .color(Palette::ACCENT()),
        )
        .padding(0)
        .style(|_, _| button::Style::default())
        .on_press_maybe((!self.busy && !files.is_empty()).then_some(all));
        let header = row![
            text(title)
                .size(typo.sz(10))
                .font(typo.regular)
                .color(Palette::TEXT_DIMMEST())
                .width(Length::Fill),
            link,
        ]
        .align_y(Alignment::Center)
        .padding([4, 8]);

        let mut rows = Column::new().push(header).spacing(1);
        for file in files {
            rows = rows.push(self.file_row(file, staged));
        }
        container(rows).padding([6, 0]).into()
    }

    fn file_row<'a>(&'a self, file: &'a ChangedFile, staged: bool) -> Element<'a, Message> {
        let typo = Typography::current();
        let size = typo.sz(12);
        let selected = self.selected.as_ref().map(|(path, s)| (path.as_str(), *s))
            == Some((&file.path, staged));

        let accent = Palette::ACCENT();
        let check = button(
            text(if staged { icons::CHECK } else { "" })
                .size(typo.sz(9))
                .font(fonts::glyph_font())
                .color(contrast_on(accent))
                .center(),
        )
        .width(16)
        .height(16)
        .padding(0)
        .on_press_maybe((!self.busy).then(|| Message::Toggle(file.path.clone(), staged)))
        .style(move |_, _| button::Style {
            background: Some(if staged { accent } else { Color::TRANSPARENT }.into()),
            border: Border::default().rounded(4).width(1).color(if staged {
                accent
            } else {
                Palette::BG_SELECTED()
            }),
            ..Default::default()
        });

        let (letter, letter_color) = kind_letter(file.kind);
        let (folder, name) = match file.path.rsplit_once('/') {
            Some((folder, name)) => (folder, name),
            None => ("", file.path.as_str()),
        };
        let mut content = row![
            check,
            text(letter)
                .size(size)
                .font(typo.bold)
                .color(letter_color)
                .width(14)
                .center(),
            text(name)
                .size(size)
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

        button(content)
            .on_press(Message::Select(file.path.clone(), staged))
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

    /// The selected file's path, its Stage or Unstage button and its "⋯" menu, then a
    /// pending discard or restore question.
    fn diff_header(&self) -> Element<'_, Message> {
        let typo = Typography::current();
        let mut header = column![];
        if let Some((path, staged)) = &self.selected {
            let (label, toggle) = if *staged {
                (
                    t("changes_unstage_file"),
                    Message::Toggle(path.clone(), true),
                )
            } else {
                (
                    t("changes_stage_file"),
                    Message::Toggle(path.clone(), false),
                )
            };
            let mut entries = vec![
                item(
                    t("changes_restore_from_branch"),
                    Message::RestoreFromBranch(Restore::Open(path.clone())),
                    false,
                ),
                item(
                    t("changes_open_file"),
                    Message::OpenFile(path.clone()),
                    false,
                ),
                item(
                    t("changes_copy_path"),
                    Message::CopyPath(path.clone()),
                    false,
                ),
            ];
            // Discarding goes back to the index version, so only work tree changes can go.
            if !*staged {
                entries.push(Entry::Separator);
                entries.push(item(
                    t("changes_discard"),
                    Message::ConfirmDiscard(Some(path.clone())),
                    true,
                ));
            }
            let menu_open = self.file_menu.as_ref() == Some(path);
            let more = menu::popup(
                gs_button::icon(
                    MORE_GLYPH,
                    Some(Message::FileMenu((!menu_open).then(|| path.clone()))),
                    t("changes_file_actions"),
                ),
                menu_open,
                entries,
                Message::FileMenu(None),
            );
            header = header.push(
                row![
                    text(path.as_str())
                        .size(typo.sz(13))
                        .font(typo.regular)
                        .color(Palette::TEXT_PRIMARY())
                        .width(Length::Fill),
                    gs_button::secondary(label, (!self.busy).then_some(toggle)),
                    more,
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .padding([10, 16]),
            );
        }
        if let Some(path) = &self.confirm_discard {
            header = header.push(question(
                vec![text(tf("changes_discard_confirm", &[("file", path)]))
                    .size(typo.sz(12))
                    .font(typo.regular)
                    .color(Palette::TEXT_SECONDARY())
                    .into()],
                gs_button::danger(
                    t("changes_discard_button"),
                    (!self.busy).then(|| Message::Discard(path.clone())),
                ),
                Message::ConfirmDiscard(None),
            ));
        }
        if let Some((path, branch)) = &self.restore {
            let branches = self
                .snapshot
                .as_ref()
                .map(|s| s.branches.clone())
                .unwrap_or_default();
            let prompt = |label: String| {
                text(label)
                    .size(typo.sz(12))
                    .font(typo.regular)
                    .color(Palette::TEXT_SECONDARY())
            };
            header = header.push(question(
                vec![
                    prompt(tf("changes_restore_prompt", &[("file", path)])).into(),
                    pick_list(branches, branch.clone(), |branch| {
                        Message::RestoreFromBranch(Restore::Branch(branch))
                    })
                    .text_size(typo.sz(12))
                    .font(typo.regular)
                    .into(),
                    prompt(t("changes_restore_warning").to_string()).into(),
                ],
                gs_button::danger(
                    t("changes_restore_button"),
                    (!self.busy && branch.is_some())
                        .then_some(Message::RestoreFromBranch(Restore::Confirm)),
                ),
                Message::RestoreFromBranch(Restore::Cancel),
            ));
        }
        column![header, horizontal_divider()].into()
    }

    fn commit_box<'a>(&'a self, snapshot: &'a Snapshot) -> Element<'a, Message> {
        let typo = Typography::current();
        let length = self.summary.chars().count();
        let counter_color = if length > SUMMARY_SOFT_LIMIT {
            Palette::WARNING()
        } else {
            Palette::TEXT_DIMMEST()
        };
        let summary = field(
            text_input(t("changes_summary_placeholder"), &self.summary)
                .on_input(Message::Summary)
                .on_submit_maybe(self.can_commit().then_some(Message::Commit)),
            Some(
                text(format!("{length} / {SUMMARY_SOFT_LIMIT}"))
                    .size(typo.sz(11))
                    .font(typo.regular)
                    .color(counter_color)
                    .into(),
            ),
        );
        let description = field(
            text_input(t("changes_description_placeholder"), &self.description)
                .on_input(Message::Description),
            None,
        );

        let staged = snapshot.staged.len();
        let mut info = vec![if staged == 1 {
            t("changes_staged_one").to_string()
        } else {
            tf("changes_staged_many", &[("count", &staged.to_string())])
        }];
        if let Some(branch) = &snapshot.branch {
            info.push(tf("changes_on_branch", &[("branch", branch)]));
        }
        if let Some(author) = &snapshot.author {
            info.push(tf("changes_author", &[("name", author)]));
        }

        let more = menu::popup(
            gs_button::icon(
                MORE_GLYPH,
                Some(Message::CommitMenu(!self.commit_menu)),
                t("changes_commit_options"),
            ),
            self.commit_menu,
            self.commit_entries(),
            Message::CommitMenu(false),
        );
        let actions = row![
            text(info.join(" · "))
                .size(typo.sz(11))
                .font(typo.regular)
                .color(Palette::TEXT_DIMMEST())
                .width(Length::Fill),
            more,
            gs_button::primary(
                t("changes_commit"),
                self.can_commit().then_some(Message::Commit)
            ),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        column![
            horizontal_divider(),
            container(column![summary, description, actions].spacing(8))
                .padding([12, 14])
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Palette::BG_SIDEBAR().into()),
                    ..Default::default()
                }),
        ]
        .into()
    }

    /// The templates, then the sign-off toggle.
    fn commit_entries(&self) -> Vec<Entry<Message>> {
        let mut entries: Vec<Entry<Message>> = TEMPLATES
            .iter()
            .enumerate()
            .map(|(index, (label, _))| {
                item(
                    &tf("changes_template", &[("name", t(label))]),
                    Message::TemplateSelected(index),
                    false,
                )
            })
            .collect();
        entries.push(Entry::Separator);
        entries.push(match self.signoff_line() {
            Some(_) => Entry::Item(MenuItem {
                label: t("changes_signoff").to_string(),
                shortcut: self.signoff.then(|| icons::CHECK.to_string()),
                on_select: Message::ToggleSignoff,
                danger: false,
            }),
            None => item(
                t("changes_signoff_missing"),
                Message::CommitMenu(false),
                false,
            ),
        });
        entries
    }
}

/// Summary, blank line, description, blank line, sign-off; empty parts are left out.
fn commit_message(summary: &str, description: &str, signoff: Option<&str>) -> String {
    [
        summary.trim(),
        description.trim(),
        signoff.unwrap_or_default(),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join("\n\n")
}

/// The change letter of a file and its colour.
fn kind_letter(kind: ChangeKind) -> (&'static str, Color) {
    match kind {
        ChangeKind::Added => ("A", Palette::SUCCESS()),
        ChangeKind::Untracked => ("U", Palette::SUCCESS()),
        ChangeKind::Modified => ("M", Palette::WARNING()),
        ChangeKind::TypeChange => ("T", Palette::WARNING()),
        ChangeKind::Deleted => ("D", Palette::ERROR()),
        ChangeKind::Conflicted => ("C", Palette::ERROR()),
        ChangeKind::Renamed => ("R", Palette::ACCENT_ICON()),
    }
}

pub(crate) fn item<M>(label: &str, on_select: M, danger: bool) -> Entry<M> {
    Entry::Item(MenuItem {
        label: label.to_string(),
        shortcut: None,
        on_select,
        danger,
    })
}

/// A confirmation band under a header: the question, Cancel, then the action.
pub(crate) fn question<'a, M: Clone + 'a>(
    prompt: Vec<Element<'a, M>>,
    action: Element<'a, M>,
    cancel: M,
) -> Element<'a, M> {
    let mut band = iced::widget::Row::with_children(prompt)
        .spacing(8)
        .align_y(Alignment::Center);
    band = band
        .push(Space::new().width(Length::Fill))
        .push(gs_button::secondary(t("changes_cancel"), Some(cancel)))
        .push(action);
    container(band)
        .padding([8, 16])
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Palette::BG_CARD().into()),
            ..Default::default()
        })
        .into()
}

/// A text input on `bg_input`, with an optional trailing element inside the field.
pub(crate) fn field<'a, M: Clone + 'a>(
    input: text_input::TextInput<'a, M>,
    trailing: Option<Element<'a, M>>,
) -> Element<'a, M> {
    let typo = Typography::current();
    let input = input
        .size(typo.sz(13))
        .font(typo.regular)
        .padding(0)
        .style(|_, _| text_input::Style {
            background: Color::TRANSPARENT.into(),
            border: Border::default(),
            icon: Palette::TEXT_DIMMER(),
            placeholder: Palette::TEXT_PLACEHOLDER(),
            value: Palette::TEXT_PRIMARY(),
            selection: Palette::ACCENT().scale_alpha(0.35),
        });
    let mut content = row![input].spacing(8).align_y(Alignment::Center);
    if let Some(trailing) = trailing {
        content = content.push(trailing);
    }
    container(content)
        .padding([8, 10])
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Palette::BG_INPUT().into()),
            border: Border::default().rounded(6),
            ..Default::default()
        })
        .into()
}

pub(crate) fn horizontal_divider<'a, M: 'a>() -> Element<'a, M> {
    container(Space::new().width(Length::Fill).height(1))
        .style(|_| container::Style {
            background: Some(Palette::DIVIDER().into()),
            ..Default::default()
        })
        .into()
}

pub(crate) fn vertical_divider<'a, M: 'a>() -> Element<'a, M> {
    container(Space::new().width(1).height(Length::Fill))
        .style(|_| container::Style {
            background: Some(Palette::DIVIDER().into()),
            ..Default::default()
        })
        .into()
}

/// Reads the changed files with their counts, the branch, the identity and the branch
/// names. No patch is built: only the selected file's is loaded.
fn read_snapshot(repo_path: &str) -> Result<Snapshot, String> {
    let changes = read_changes(repo_path).map_err(|err| err.to_string())?;
    let branch = read_repo_status(repo_path).ok().and_then(|s| s.branch);
    let signature = Repository::open(repo_path)
        .and_then(|repo| repo.signature().map(|sig| sig.to_owned()))
        .ok();
    let author = signature
        .as_ref()
        .and_then(|sig| sig.name().ok().map(str::to_string));
    let signoff_line = signature.as_ref().map(|sig| {
        format!(
            "Signed-off-by: {} <{}>",
            sig.name().unwrap_or_default(),
            sig.email().unwrap_or_default()
        )
    });
    let branches = list_branches(repo_path)
        .map(|branches| branches.into_iter().map(|branch| branch.name).collect())
        .unwrap_or_default();
    Ok(Snapshot {
        staged: changes.staged,
        unstaged: changes.unstaged,
        branch,
        author,
        signoff_line,
        branches,
    })
}

/// Stages files of the repository: the content of each one that exists in the work tree,
/// otherwise its deletion. Paths are relative to the repository root, not to the process's
/// current folder. The index is written once.
fn stage_paths(repo_path: &str, paths: &[String]) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str(t("repo_no_work_tree")))?;
    let mut index = repo.index()?;
    for path in paths {
        let path = Path::new(path);
        // symlink_metadata: a dangling symlink is still a file to add.
        if workdir.join(path).symlink_metadata().is_ok() {
            index.add_path(path)?;
        } else {
            index.remove_path(path)?;
        }
    }
    index.write()
}

/// Puts the index entries of `paths` back to HEAD, or removes them before the first commit.
fn unstage_paths(repo_path: &str, paths: &[String]) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let head = match repo.head() {
        Ok(head) => Some(head.peel(ObjectType::Commit)?),
        Err(err) if err.code() == git2::ErrorCode::UnbornBranch => None,
        Err(err) => return Err(err),
    };
    repo.reset_default(head.as_ref(), paths.iter().map(Path::new))
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Status;

    fn repo_with_identity() -> (tempfile::TempDir, Repository) {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test User").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();
        (dir, repo)
    }

    fn paths(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn stage_path_uses_the_repo_workdir() {
        let (dir, repo) = repo_with_identity();
        let repo_path = dir.path().to_str().unwrap();

        // Cargo runs tests from the crate root, which has a Cargo.toml and no notes file.
        // Code that checked these names in the process's folder got both of them wrong.
        let modified = "only-in-the-test-repo.txt";
        let deleted = "Cargo.toml";
        let workdir = dir.path();
        std::fs::write(workdir.join(modified), "one").unwrap();
        std::fs::write(workdir.join(deleted), "[package]").unwrap();
        stage_paths(repo_path, &paths(&[modified, deleted])).unwrap();
        commit_index(repo_path, "initial").unwrap();

        std::fs::write(workdir.join(modified), "two").unwrap();
        std::fs::remove_file(workdir.join(deleted)).unwrap();
        stage_paths(repo_path, &paths(&[modified, deleted])).unwrap();

        let status = |name| repo.status_file(Path::new(name)).unwrap();
        assert_eq!(status(modified), Status::INDEX_MODIFIED);
        assert_eq!(status(deleted), Status::INDEX_DELETED);

        // Unstaging puts the HEAD version back in the index; the work tree keeps the change.
        unstage_paths(repo_path, &paths(&[modified, deleted])).unwrap();
        assert_eq!(status(modified), Status::WT_MODIFIED);
        assert_eq!(status(deleted), Status::WT_DELETED);
    }

    #[test]
    fn unstaging_before_the_first_commit_leaves_the_file_untracked() {
        let (dir, repo) = repo_with_identity();
        let repo_path = dir.path().to_str().unwrap();
        std::fs::write(dir.path().join("new.txt"), "x").unwrap();
        stage_paths(repo_path, &paths(&["new.txt"])).unwrap();
        unstage_paths(repo_path, &paths(&["new.txt"])).unwrap();
        assert_eq!(
            repo.status_file(Path::new("new.txt")).unwrap(),
            Status::WT_NEW
        );
    }

    #[test]
    fn commit_message_joins_summary_description_and_signoff() {
        let signoff = "Signed-off-by: Test User <test@example.com>";
        assert_eq!(
            commit_message(" feat: add X ", "Why X.\n", Some(signoff)),
            format!("feat: add X\n\nWhy X.\n\n{signoff}")
        );
        assert_eq!(commit_message("fix: y", "  ", None), "fix: y");
        assert_eq!(
            commit_message("fix: y", "", Some(signoff)),
            format!("fix: y\n\n{signoff}")
        );
    }

    fn changed_file(path: &str) -> ChangedFile {
        ChangedFile {
            path: path.to_string(),
            old_path: None,
            kind: ChangeKind::Modified,
            additions: 1,
            deletions: 0,
            binary: false,
        }
    }

    #[test]
    fn commit_disabled_without_summary_or_staged_files() {
        let mut panel = ChangesPanel {
            repo: Some("/repo".into()),
            snapshot: Some(Snapshot::default()),
            summary: "feat: add X".to_string(),
            ..ChangesPanel::default()
        };
        assert!(!panel.can_commit(), "nothing staged");

        panel.snapshot = Some(Snapshot {
            staged: vec![changed_file("a.txt")],
            ..Snapshot::default()
        });
        assert!(panel.can_commit());

        panel.summary = "   ".to_string();
        assert!(!panel.can_commit(), "blank summary");

        panel.summary = "feat: add X".to_string();
        panel.busy = true;
        assert!(!panel.can_commit(), "an action is running");
    }

    #[test]
    fn a_reload_keeps_the_shown_diff_until_the_new_one_arrives() {
        let snapshot = || Snapshot {
            unstaged: vec![changed_file("a.txt"), changed_file("b.txt")],
            ..Snapshot::default()
        };
        let mut panel = ChangesPanel {
            repo: Some("/repo".into()),
            ..ChangesPanel::default()
        };
        let _ = panel.apply_snapshot(snapshot());
        assert!(matches!(panel.diff, Some(Diff::Loading)));

        panel.diff = Some(Diff::Empty);
        let _ = panel.apply_snapshot(snapshot());
        assert!(
            matches!(panel.diff, Some(Diff::Empty)),
            "same file selected"
        );

        panel.selected = Some(("gone.txt".to_string(), false));
        let _ = panel.apply_snapshot(snapshot());
        assert!(matches!(panel.diff, Some(Diff::Loading)), "selection moved");
    }

    #[test]
    fn reloading_the_same_repository_keeps_the_drafts() {
        let (dir, _repo) = repo_with_identity();
        let context = RepoContext::from_path(dir.path()).unwrap();
        let mut panel = ChangesPanel::default();
        let _ = panel.load(&context);
        panel.summary = "feat: draft".to_string();
        panel.description = "body".to_string();
        let _ = panel.load(&context);
        assert_eq!(panel.summary, "feat: draft");
        assert_eq!(panel.description, "body");

        let other = tempfile::tempdir().unwrap();
        Repository::init(other.path()).unwrap();
        let _ = panel.load(&RepoContext::from_path(other.path()).unwrap());
        assert!(panel.summary.is_empty());
    }

    #[test]
    fn snapshot_lists_changes_and_reads_the_identity() {
        let (dir, _repo) = repo_with_identity();
        std::fs::write(dir.path().join("new.txt"), "x\ny\n").unwrap();

        let snapshot = read_snapshot(dir.path().to_str().unwrap()).unwrap();
        assert!(snapshot.staged.is_empty());
        assert_eq!(snapshot.unstaged[0].path, "new.txt");
        assert_eq!(snapshot.unstaged[0].additions, 2);
        assert_eq!(snapshot.author.as_deref(), Some("Test User"));
        assert_eq!(
            snapshot.signoff_line.as_deref(),
            Some("Signed-off-by: Test User <test@example.com>")
        );
    }

    #[test]
    fn a_staged_file_stays_selected_in_its_new_group() {
        let mut panel = ChangesPanel {
            repo: Some("/repo".into()),
            selected: Some(("a.txt".to_string(), false)),
            ..ChangesPanel::default()
        };
        let _ = panel.apply_snapshot(Snapshot {
            staged: vec![changed_file("a.txt")],
            unstaged: vec![changed_file("a.txt"), changed_file("b.txt")],
            ..Snapshot::default()
        });
        assert_eq!(panel.selected, Some(("a.txt".to_string(), false)));
        assert_eq!(panel.count(), 2, "a file in both groups counts once");

        let _ = panel.apply_snapshot(Snapshot {
            staged: vec![changed_file("a.txt")],
            unstaged: vec![changed_file("b.txt")],
            ..Snapshot::default()
        });
        assert_eq!(panel.selected, Some(("a.txt".to_string(), true)));
    }
}
