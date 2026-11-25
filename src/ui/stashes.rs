//! The Stashes tab: a bar that stashes the working tree, one card per stash with Apply and
//! a "⋯" menu, and the files and diff of the stash whose contents are shown. Git reads and
//! actions run in background tasks; `view` only reads the panel's state.

use std::sync::Arc;

use colony_ui::theme::{contrast_on, Palette};
use colony_ui::widgets::icons;
use colony_ui::{fonts, Typography};
use iced::widget::{button, column, container, row, scrollable, text, text_input, Column, Space};
use iced::{Alignment, Border, Element, Length, Task};

use crate::git::diff::FileDiffSummary;
use crate::git::stash::{
    apply_stash, count_stashable, create_stash, drop_stash, list_stashes, stash_file_patch,
    stash_files, StashEntry,
};
use crate::i18n::{t, tf};
use crate::ui::changes::{field, horizontal_divider, item, question, vertical_divider};
use crate::ui::context::{git_task, RepoContext};
use crate::ui::diff_view::{self, Diff};
use crate::ui::history::relative_time;
use crate::ui::widgets::button as gs_button;
use crate::ui::widgets::menu::{self, Entry};
use crate::ui::widgets::toast::Toast;

const MORE_GLYPH: &str = "\u{f141}";
/// Width of the stash list while a stash's contents are shown.
const LIST_WIDTH: f32 = 420.0;
/// The failure title of "Stash changes"; its success also clears the message field.
const CREATE_FAILED: &str = "stashes_create_failed";

/// The stashes and how many files a stash made now would take.
#[derive(Debug, Clone)]
pub struct Snapshot {
    stashes: Vec<StashEntry>,
    stashable: usize,
}

/// A step of "Apply and drop".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Apply,
    Drop,
}

#[derive(Debug, Clone)]
#[expect(
    clippy::enum_variant_names,
    reason = "`Message(String)` edits the stash's message, the plan's name for it"
)]
pub enum Message {
    /// A read of the stashes, numbered so that only the latest one is shown.
    Loaded(Arc<str>, u64, Result<Snapshot, String>),
    /// The new stash's message was edited.
    Message(String),
    ToggleUntracked,
    Create,
    Apply(usize, String),
    ApplyAndDrop(usize, String),
    /// Asks to confirm dropping a stash, or closes the question.
    ConfirmDrop(Option<(usize, String)>),
    Drop(usize, String),
    /// Opens the "⋯" menu of the stash with this id, or closes it.
    Menu(Option<String>),
    /// Shows the files and diff of a stash.
    Show(usize, String),
    CloseContents,
    FilesLoaded(Arc<str>, String, Result<Vec<FileDiffSummary>, String>),
    SelectFile(String),
    /// The patch of `(stash id, file)`.
    PatchLoaded(
        Arc<str>,
        (String, String),
        Result<Option<(String, bool)>, String>,
    ),
    /// The success message, or the error; the key names the failure title.
    Done(Arc<str>, &'static str, Result<String, String>),
}

pub struct StashesPanel {
    repo: Option<Arc<str>>,
    stashes: Vec<StashEntry>,
    stashable: usize,
    loaded: bool,
    error: Option<String>,
    /// Number of the last read; older replies are dropped.
    request: u64,
    message: String,
    include_untracked: bool,
    menu: Option<String>,
    confirm_drop: Option<(usize, String)>,
    busy: bool,
    /// The stash whose contents are shown, by index and id.
    shown: Option<(usize, String)>,
    files: Vec<FileDiffSummary>,
    files_error: Option<String>,
    selected_file: Option<String>,
    diff: Option<Diff>,
}

impl Default for StashesPanel {
    fn default() -> Self {
        Self {
            repo: None,
            stashes: Vec::new(),
            stashable: 0,
            loaded: false,
            error: None,
            request: 0,
            message: String::new(),
            include_untracked: true,
            menu: None,
            confirm_drop: None,
            busy: false,
            shown: None,
            files: Vec::new(),
            files_error: None,
            selected_file: None,
            diff: None,
        }
    }
}

impl StashesPanel {
    /// Reads the stashes of `repo` in the background. Reloading the same repository keeps
    /// the message draft, the untracked toggle and the shown stash.
    pub fn load(&mut self, repo: &RepoContext) -> Task<Message> {
        if !self.is_current(repo.path()) {
            // Keep request numbers growing so a reply from before the reset is dropped.
            let request = self.request;
            *self = Self {
                repo: Some(repo.path_arc()),
                request,
                ..Self::default()
            };
        }
        self.refresh()
    }

    /// The number of stashes, shown on the Stashes tab.
    pub fn count(&self) -> usize {
        self.stashes.len()
    }

    /// Handles a message; the toast, if any, is for the application to show.
    pub fn update(&mut self, message: Message) -> (Task<Message>, Option<Toast>) {
        match message {
            Message::Loaded(repo, request, result) => {
                if self.is_current(&repo) && request == self.request {
                    self.loaded = true;
                    match result {
                        Ok(snapshot) => self.apply_snapshot(snapshot),
                        Err(err) => self.error = Some(err),
                    }
                }
            }
            Message::Message(value) => self.message = value,
            Message::ToggleUntracked => {
                self.include_untracked = !self.include_untracked;
                return (self.refresh(), None);
            }
            Message::Create => {
                if self.stashable == 0 {
                    return (Task::none(), None);
                }
                let message = self.message.trim().to_string();
                let include_untracked = self.include_untracked;
                let task = self.run(CREATE_FAILED, move |repo| {
                    create_stash(repo, &message, include_untracked)
                        .map(|()| t("stashes_created").to_string())
                        .map_err(|err| err.to_string())
                });
                return (task, None);
            }
            Message::Apply(index, id) => {
                self.menu = None;
                let task = self.run("stashes_apply_failed", move |repo| {
                    apply_stash(repo, index, &id)
                        .map(|conflicts| {
                            let key = if conflicts {
                                "stashes_applied_conflicts"
                            } else {
                                "stashes_applied"
                            };
                            tf(key, &[("index", &index.to_string())])
                        })
                        .map_err(|err| err.to_string())
                });
                return (task, None);
            }
            Message::ApplyAndDrop(index, id) => {
                self.menu = None;
                let task = self.run("stashes_apply_drop_failed", move |repo| {
                    apply_and_drop(repo, index, &id)
                });
                return (task, None);
            }
            Message::ConfirmDrop(target) => {
                self.menu = None;
                self.confirm_drop = target;
            }
            Message::Drop(index, id) => {
                self.confirm_drop = None;
                let task = self.run("stashes_drop_failed", move |repo| {
                    drop_stash(repo, index, &id)
                        .map(|()| tf("stashes_dropped", &[("index", &index.to_string())]))
                        .map_err(|err| err.to_string())
                });
                return (task, None);
            }
            Message::Menu(id) => self.menu = id,
            Message::Show(index, id) => return (self.show(index, id), None),
            Message::CloseContents => self.close_contents(),
            Message::FilesLoaded(repo, id, result) => {
                if self.is_current(&repo) && self.shown_id() == Some(id.as_str()) {
                    match result {
                        Ok(files) => {
                            self.files = files;
                            match self.files.first() {
                                Some(file) => {
                                    let path = file.path.clone();
                                    return (self.select_file(path), None);
                                }
                                None => self.diff = Some(Diff::Empty),
                            }
                        }
                        Err(err) => {
                            self.files_error = Some(err);
                            self.diff = None;
                            // The stash list may have changed under the shown stash.
                            return (self.refresh(), None);
                        }
                    }
                }
            }
            Message::SelectFile(path) => return (self.select_file(path), None),
            Message::PatchLoaded(repo, (id, path), result) => {
                if self.is_current(&repo)
                    && self.shown_id() == Some(id.as_str())
                    && self.selected_file.as_ref() == Some(&path)
                {
                    let failed = result.is_err();
                    self.diff = Some(Diff::loaded(result));
                    if failed {
                        return (self.refresh(), None);
                    }
                }
            }
            Message::Done(repo, failed, result) => {
                if self.is_current(&repo) {
                    self.busy = false;
                    let toast = match result {
                        Ok(message) => {
                            if failed == CREATE_FAILED {
                                self.message.clear();
                            }
                            Toast::success(message)
                        }
                        Err(err) => Toast::error(t(failed), err),
                    };
                    // Also reloads after a "list changed" error.
                    return (self.refresh(), Some(toast));
                }
            }
        }
        (Task::none(), None)
    }

    fn is_current(&self, repo: &str) -> bool {
        self.repo.as_deref() == Some(repo)
    }

    fn shown_id(&self) -> Option<&str> {
        self.shown.as_ref().map(|(_, id)| id.as_str())
    }

    fn refresh(&mut self) -> Task<Message> {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        self.request += 1;
        let request = self.request;
        let include_untracked = self.include_untracked;
        git_task(
            repo,
            move |repo| read_snapshot(repo, include_untracked),
            move |repo, result| Message::Loaded(repo, request, result),
        )
    }

    /// Takes the new list. A stash still listed keeps its menu, question and contents at
    /// its new index; those of a stash that is gone close.
    fn apply_snapshot(&mut self, snapshot: Snapshot) {
        self.error = None;
        self.stashes = snapshot.stashes;
        self.stashable = snapshot.stashable;
        let index_of = |id: &str| {
            self.stashes
                .iter()
                .find(|stash| stash.id == id)
                .map(|stash| stash.index)
        };
        self.confirm_drop = self
            .confirm_drop
            .take()
            .and_then(|(_, id)| Some((index_of(&id)?, id)));
        if self
            .menu
            .as_deref()
            .is_some_and(|id| index_of(id).is_none())
        {
            self.menu = None;
        }
        if let Some((_, id)) = self.shown.take() {
            match index_of(&id) {
                Some(index) => self.shown = Some((index, id)),
                None => self.close_contents(),
            }
        }
    }

    fn show(&mut self, index: usize, id: String) -> Task<Message> {
        self.menu = None;
        self.close_contents();
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        self.shown = Some((index, id.clone()));
        self.diff = Some(Diff::Loading);
        let job_id = id.clone();
        git_task(
            repo,
            move |repo| stash_files(repo, index, &job_id).map_err(|err| err.to_string()),
            move |repo, result| Message::FilesLoaded(repo, id.clone(), result),
        )
    }

    fn close_contents(&mut self) {
        self.shown = None;
        self.files.clear();
        self.files_error = None;
        self.selected_file = None;
        self.diff = None;
    }

    fn select_file(&mut self, path: String) -> Task<Message> {
        let (Some(repo), Some((index, id))) = (self.repo.clone(), self.shown.clone()) else {
            return Task::none();
        };
        self.selected_file = Some(path.clone());
        self.diff = Some(Diff::Loading);
        let key = (id.clone(), path.clone());
        git_task(
            repo,
            move |repo| {
                stash_file_patch(repo, index, &id, &path)
                    .map(|diff| diff.map(|diff| (diff.patch, diff.truncated)))
                    .map_err(|err| err.to_string())
            },
            move |repo, result| Message::PatchLoaded(repo, key.clone(), result),
        )
    }

    /// Runs a git action unless one is running; the list is read again when it ends.
    fn run(
        &mut self,
        failed: &'static str,
        job: impl FnOnce(&str) -> Result<String, String> + Send + 'static,
    ) -> Task<Message> {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        if self.busy {
            return Task::none();
        }
        self.busy = true;
        git_task(repo, job, move |repo, result| {
            Message::Done(repo, failed, result)
        })
    }

    pub fn view(&self) -> Element<'_, Message> {
        let list = column![self.new_stash_bar(), self.cards()]
            .spacing(14)
            .padding([18, 22])
            .height(Length::Fill);
        if self.shown.is_none() {
            return list.width(Length::Fill).into();
        }
        row![
            list.width(Length::Fixed(LIST_WIDTH)),
            vertical_divider(),
            self.contents()
        ]
        .height(Length::Fill)
        .into()
    }

    /// The message field, the untracked toggle and "Stash changes", then the number of
    /// files a stash would take.
    fn new_stash_bar(&self) -> Element<'_, Message> {
        let typo = Typography::current();
        let ready = !self.busy && self.stashable > 0;
        let input = field(
            text_input(t("stashes_message_placeholder"), &self.message)
                .on_input(Message::Message)
                .on_submit_maybe(ready.then_some(Message::Create)),
            None,
        );
        let bar = container(
            row![
                input,
                self.untracked_toggle(),
                gs_button::primary(t("stashes_create"), ready.then_some(Message::Create)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding(10)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Palette::BG_SIDEBAR().into()),
            border: Border::default().rounded(10),
            ..Default::default()
        });
        let count = match self.stashable {
            0 => t("stashes_nothing_to_stash").to_string(),
            1 => t("stashes_will_stash_one").to_string(),
            count => tf("stashes_will_stash_many", &[("count", &count.to_string())]),
        };
        let mut content = column![bar].spacing(8);
        if self.loaded {
            content = content.push(
                text(count)
                    .size(typo.sz(12))
                    .font(typo.regular)
                    .color(Palette::TEXT_DIMMER()),
            );
        }
        content.into()
    }

    /// "Include untracked files" with a check box drawn like the Changes tab's.
    fn untracked_toggle(&self) -> Element<'_, Message> {
        let typo = Typography::current();
        let on = self.include_untracked;
        let accent = Palette::ACCENT();
        let check = container(
            text(if on { icons::CHECK } else { "" })
                .size(typo.sz(9))
                .font(fonts::glyph_font())
                .color(contrast_on(accent))
                .center(),
        )
        .width(14)
        .height(14)
        .style(move |_| container::Style {
            background: on.then(|| accent.into()),
            border: Border::default().rounded(4).width(1).color(if on {
                accent
            } else {
                Palette::BG_SELECTED()
            }),
            ..Default::default()
        });
        button(
            row![
                check,
                text(t("stashes_include_untracked"))
                    .size(typo.sz(12))
                    .font(typo.regular)
                    .color(Palette::TEXT_DIM()),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([4, 6])
        .on_press_maybe((!self.busy).then_some(Message::ToggleUntracked))
        .style(|_, _| button::Style::default())
        .into()
    }

    /// One card per stash, each followed by its drop question when open; or the empty
    /// state.
    fn cards(&self) -> Element<'_, Message> {
        let typo = Typography::current();
        if let Some(err) = &self.error {
            return note_line(tf("stashes_load_failed", &[("error", err)]));
        }
        if self.loaded && self.stashes.is_empty() {
            return container(
                column![
                    text(t("stashes_empty_title"))
                        .size(typo.sz(15))
                        .font(typo.bold)
                        .color(Palette::TEXT_PRIMARY()),
                    text(t("stashes_empty_detail"))
                        .size(typo.sz(12))
                        .font(typo.regular)
                        .color(Palette::TEXT_DIMMER()),
                ]
                .spacing(6)
                .align_x(Alignment::Center),
            )
            .center(Length::Fill)
            .into();
        }
        let now = chrono::Utc::now().timestamp();
        let mut cards = Column::new().spacing(8);
        for stash in &self.stashes {
            cards = cards.push(self.card(stash, now));
            if let Some((index, id)) = &self.confirm_drop {
                if *id == stash.id {
                    cards = cards.push(question(
                        vec![text(tf(
                            "stashes_drop_confirm",
                            &[
                                ("index", &index.to_string()),
                                ("message", stash_title(&stash.message)),
                            ],
                        ))
                        .size(typo.sz(12))
                        .font(typo.regular)
                        .color(Palette::TEXT_SECONDARY())
                        .into()],
                        gs_button::danger(
                            t("stashes_drop_button"),
                            (!self.busy).then(|| Message::Drop(*index, id.clone())),
                        ),
                        Message::ConfirmDrop(None),
                    ));
                }
            }
        }
        scrollable(cards).height(Length::Fill).into()
    }

    /// "#0", the message over "on <branch> · <date> · N files", Apply and "⋯".
    fn card<'a>(&'a self, stash: &'a StashEntry, now: i64) -> Element<'a, Message> {
        let typo = Typography::current();
        let (index, id) = (stash.index, &stash.id);
        let mut meta = vec![match &stash.branch {
            Some(branch) => tf("stashes_on_branch", &[("branch", branch)]),
            None => t("stashes_detached").to_string(),
        }];
        meta.push(relative_time(stash.time, now));
        meta.push(if stash.file_count == 1 {
            t("stashes_one_file").to_string()
        } else {
            tf("stashes_files", &[("count", &stash.file_count.to_string())])
        });

        let menu_open = self.menu.as_ref() == Some(id);
        let entries = vec![
            item(
                t("stashes_show_contents"),
                Message::Show(index, id.clone()),
                false,
            ),
            item(
                t("stashes_apply_and_drop"),
                Message::ApplyAndDrop(index, id.clone()),
                false,
            ),
            Entry::Separator,
            item(
                t("stashes_drop"),
                Message::ConfirmDrop(Some((index, id.clone()))),
                true,
            ),
        ];
        let more = menu::popup(
            gs_button::icon(
                MORE_GLYPH,
                Some(Message::Menu((!menu_open).then(|| id.clone()))),
                t("stashes_actions"),
            ),
            menu_open,
            entries,
            Message::Menu(None),
        );
        let shown = self.shown_id() == Some(id.as_str());
        container(
            row![
                text(format!("#{index}"))
                    .size(typo.sz(11))
                    .font(typo.regular)
                    .color(Palette::TEXT_DIMMEST())
                    .width(28),
                column![
                    text(stash_title(&stash.message))
                        .size(typo.sz(13))
                        .font(typo.regular)
                        .color(Palette::TEXT_PRIMARY()),
                    text(meta.join(" · "))
                        .size(typo.sz(11))
                        .font(typo.regular)
                        .color(Palette::TEXT_DIMMER()),
                ]
                .spacing(3)
                .width(Length::Fill),
                gs_button::secondary(
                    t("stashes_apply"),
                    (!self.busy).then(|| Message::Apply(index, id.clone())),
                ),
                more,
            ]
            .spacing(14)
            .align_y(Alignment::Center),
        )
        .padding([12, 14])
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(
                if shown {
                    Palette::BG_CARD_HOVER()
                } else {
                    Palette::BG_SIDEBAR()
                }
                .into(),
            ),
            border: Border::default().rounded(10),
            ..Default::default()
        })
        .into()
    }

    /// The shown stash's title with a close button, its files, then the selected file's diff.
    fn contents(&self) -> Element<'_, Message> {
        let typo = Typography::current();
        let Some((index, id)) = &self.shown else {
            return Space::new().into();
        };
        let title = self
            .stashes
            .iter()
            .find(|stash| &stash.id == id)
            .map_or("", |stash| stash_title(&stash.message));
        let header = row![
            text(format!("#{index}  {title}"))
                .size(typo.sz(13))
                .font(typo.regular)
                .color(Palette::TEXT_PRIMARY())
                .width(Length::Fill),
            gs_button::icon(
                icons::CLOSE,
                Some(Message::CloseContents),
                t("stashes_close_contents"),
            ),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .padding([10, 16]);

        let mut files = Column::new().spacing(1);
        if let Some(err) = &self.files_error {
            files = files.push(note_line(err.clone()));
        }
        for file in &self.files {
            files = files.push(self.file_row(file));
        }
        column![
            header,
            horizontal_divider(),
            container(scrollable(files.padding([6, 10]))).max_height(180.0),
            horizontal_divider(),
            diff_view::pane(self.diff.as_ref(), t("stashes_select_file")),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// A file of the shown stash: its name and folder, then +N −N.
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

/// Applies the stash, then drops it unless the apply left conflicts, as `git stash pop` does.
fn apply_and_drop(repo: &str, index: usize, id: &str) -> Result<String, String> {
    apply_then_drop(|step| match step {
        Step::Apply => match apply_stash(repo, index, id) {
            Ok(false) => Ok(()),
            Ok(true) => Err(t("stashes_conflicts_kept").to_string()),
            Err(err) => Err(err.to_string()),
        },
        Step::Drop => drop_stash(repo, index, id).map_err(|err| err.to_string()),
    })
    .map(|()| tf("stashes_applied_dropped", &[("index", &index.to_string())]))
}

/// Applies a stash, then drops it. The drop runs only once the apply succeeded, so a
/// failed apply keeps the stash.
fn apply_then_drop(mut run: impl FnMut(Step) -> Result<(), String>) -> Result<(), String> {
    run(Step::Apply)?;
    run(Step::Drop).map_err(|err| tf("stashes_applied_not_dropped", &[("error", &err)]))
}

/// The message without git's "On <branch>: " or "WIP on <branch>: " prefix, which the
/// card's second line already shows. Branch names cannot contain ':'.
fn stash_title(message: &str) -> &str {
    if message.starts_with("On ") || message.starts_with("WIP on ") {
        if let Some((_, rest)) = message.split_once(": ") {
            return rest;
        }
    }
    message
}

fn read_snapshot(repo: &str, include_untracked: bool) -> Result<Snapshot, String> {
    let stashes = list_stashes(repo).map_err(|err| err.to_string())?;
    let stashable = count_stashable(repo, include_untracked).map_err(|err| err.to_string())?;
    Ok(Snapshot { stashes, stashable })
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

    #[test]
    fn apply_and_drop_runs_drop_only_after_apply_succeeds() {
        let mut steps = Vec::new();
        let result = apply_then_drop(|step| {
            steps.push(step);
            Ok(())
        });
        assert!(result.is_ok());
        assert_eq!(steps, [Step::Apply, Step::Drop]);

        let mut steps = Vec::new();
        let result = apply_then_drop(|step| {
            steps.push(step);
            match step {
                Step::Apply => Err("conflict".to_string()),
                Step::Drop => Ok(()),
            }
        });
        assert_eq!(result, Err("conflict".to_string()));
        assert_eq!(steps, [Step::Apply], "a failed apply keeps the stash");

        let result = apply_then_drop(|step| match step {
            Step::Apply => Ok(()),
            Step::Drop => Err("locked".to_string()),
        });
        let err = result.unwrap_err();
        assert!(err.contains("locked"), "{err}");
        assert_ne!(err, "locked", "says the stash was applied");
    }

    #[test]
    fn apply_and_drop_keeps_a_stash_that_left_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let mut options = git2::RepositoryInitOptions::new();
        options.initial_head("main");
        let repo = git2::Repository::init_opts(dir.path(), &options).unwrap();
        let path = dir.path().to_str().unwrap();
        let file = dir.path().join("a.txt");
        let commit = |contents: &str| {
            std::fs::write(&file, contents).unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(std::path::Path::new("a.txt")).unwrap();
            index.write().unwrap();
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            let sig = git2::Signature::now("Tester", "tester@example.com").unwrap();
            let parent = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
            let parents: Vec<_> = parent.iter().collect();
            repo.commit(Some("HEAD"), &sig, &sig, "edit", &tree, &parents)
                .unwrap();
        };
        commit("base");
        std::fs::write(&file, "stashed").unwrap();
        create_stash(path, "a", false).unwrap();
        commit("committed");

        let stash = list_stashes(path).unwrap().remove(0);
        let err = apply_and_drop(path, stash.index, &stash.id).unwrap_err();

        assert_eq!(err, t("stashes_conflicts_kept"));
        assert_eq!(list_stashes(path).unwrap().len(), 1, "the stash is kept");
    }

    #[test]
    fn stash_title_drops_the_branch_prefix() {
        assert_eq!(
            stash_title("On main: WIP badge colours"),
            "WIP badge colours"
        );
        assert_eq!(
            stash_title("WIP on feature/x: 1a2b3c4 summary"),
            "1a2b3c4 summary"
        );
        assert_eq!(stash_title("custom: message"), "custom: message");
    }
}
