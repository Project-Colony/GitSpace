//! The repository header: name, branch and divergence chips, folder and terminal buttons,
//! Fetch, Pull and Push, the remotes menu, and the section tabs below it.

use std::ffi::OsStr;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command as ProcessCommand;
use std::sync::Arc;

use colony_ui::theme::Palette;
use colony_ui::Typography;
use iced::widget::{column, container, row, text, Space};
use iced::{Alignment, Element, Length, Padding, Task};

use crate::auth::AuthManager;
use crate::config::NetworkOptions;
use crate::git::remote::{
    fetch_remote, list_remotes, pull_branch, push_branch, PullOutcome, RemoteInfo,
};
use crate::git::status::{read_repo_status, RepoStatus};
use crate::i18n::{t, tf};
use crate::ui::context::RepoContext;
use crate::ui::widgets::button;
use crate::ui::widgets::chip::{chip, Tone};
use crate::ui::widgets::menu::{self, Entry, MenuItem};
use crate::ui::widgets::tabs::tabs;
use crate::ui::widgets::toast::Toast;

const FOLDER_GLYPH: &str = "\u{f07c}";
const TERMINAL_GLYPH: &str = "\u{f120}";
const MORE_GLYPH: &str = "\u{f141}";

/// The sections of an open repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoTab {
    Changes,
    History,
    Branches,
    Stashes,
}

impl RepoTab {
    pub const ALL: [RepoTab; 4] = [
        RepoTab::Changes,
        RepoTab::History,
        RepoTab::Branches,
        RepoTab::Stashes,
    ];

    fn label(self) -> &'static str {
        match self {
            RepoTab::Changes => t("tab_changes"),
            RepoTab::History => t("tab_history"),
            RepoTab::Branches => t("tab_branches"),
            RepoTab::Stashes => t("tab_stashes"),
        }
    }
}

/// The counts shown after the Changes and Stashes tab labels.
#[derive(Debug, Clone, Copy, Default)]
pub struct TabCounts {
    pub changes: usize,
    pub stashes: usize,
}

/// A network operation on one of the repository's remotes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteAction {
    Fetch,
    /// The fetch the auto-fetch timer starts: only a failure is reported.
    AutoFetch,
    Pull,
    Push,
}

impl RemoteAction {
    fn label(self) -> &'static str {
        match self {
            RemoteAction::Fetch | RemoteAction::AutoFetch => t("header_fetch"),
            RemoteAction::Pull => t("header_pull"),
            RemoteAction::Push => t("header_push"),
        }
    }

    fn progress(self) -> &'static str {
        match self {
            RemoteAction::Fetch | RemoteAction::AutoFetch => t("header_fetching"),
            RemoteAction::Pull => t("header_pulling"),
            RemoteAction::Push => t("header_pushing"),
        }
    }

    fn failed(self) -> &'static str {
        match self {
            RemoteAction::Fetch => t("header_fetch_failed"),
            RemoteAction::AutoFetch => t("header_auto_fetch_failed"),
            RemoteAction::Pull => t("header_pull_failed"),
            RemoteAction::Push => t("header_push_failed"),
        }
    }
}

/// The button drawn in the accent colour: Push when there is something to push, else Pull
/// when there is something to pull.
fn primary_action(ahead: usize, behind: usize) -> Option<RemoteAction> {
    if ahead > 0 {
        Some(RemoteAction::Push)
    } else if behind > 0 {
        Some(RemoteAction::Pull)
    } else {
        None
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Status and remotes read for the repository `path`.
    StateLoaded {
        path: Arc<str>,
        status: Result<RepoStatus, String>,
        remotes: Result<Vec<RemoteInfo>, String>,
    },
    /// Fetch one remote from the remotes menu, or the upstream's remote with `None`.
    Fetch(Option<String>),
    Pull,
    Push,
    /// A network operation on the repository `path` ended.
    ActionFinished {
        path: Arc<str>,
        action: RemoteAction,
        result: Result<String, String>,
    },
    OpenFolder,
    OpenTerminal,
    /// Opens or closes the remotes menu.
    RemotesMenu(bool),
    /// Handled by the application, which owns the selected tab.
    SelectTab(RepoTab),
}

/// The remote and branches a network operation works on.
#[derive(Debug, Clone)]
struct RemoteSelection {
    remote_name: String,
    /// URL used to look up the token, `None` when the remote has none.
    url: Option<String>,
    /// The checked-out branch.
    branch: Option<String>,
    /// The branch it tracks on the remote, when an upstream is set.
    upstream_branch: Option<String>,
}

pub struct RepoHeader {
    repo: Option<RepoContext>,
    status: Option<RepoStatus>,
    remotes: Vec<RemoteInfo>,
    network: NetworkOptions,
    /// The status and remotes of a newly opened repository are being read; nothing runs
    /// before they are known.
    loading: bool,
    /// The network operation running on `repo`.
    running: Option<RemoteAction>,
    menu_open: bool,
}

impl RepoHeader {
    pub fn new(network: NetworkOptions) -> Self {
        Self {
            repo: None,
            status: None,
            remotes: Vec::new(),
            network,
            loading: false,
            running: None,
            menu_open: false,
        }
    }

    pub fn set_network_preferences(&mut self, network: NetworkOptions) {
        self.network = network;
    }

    /// True while the state loads or a network operation runs on the open repository.
    pub fn is_busy(&self) -> bool {
        self.loading || self.running.is_some()
    }

    /// Reads the status and remotes of `repo` off the UI thread. Another repository starts
    /// from an empty state. Reading the open one again keeps the buttons usable and leaves
    /// the auto-fetch timer running, so a reload on window focus does not delay auto-fetch.
    pub fn load(&mut self, repo: &RepoContext) -> Task<Message> {
        if self.repo.as_ref().map(RepoContext::path) != Some(repo.path()) {
            *self = Self::new(self.network.clone());
            self.repo = Some(repo.clone());
            self.loading = true;
        }

        let path = repo.path_arc();
        Task::perform(
            async move {
                let repo_path = Arc::clone(&path);
                let (status, remotes) = tokio::task::spawn_blocking(move || {
                    let status = read_repo_status(&*repo_path).map_err(|err| err.to_string());
                    let remotes = list_remotes(&*repo_path).map_err(|err| err.to_string());
                    (status, remotes)
                })
                .await
                .unwrap_or_else(|err| (Err(err.to_string()), Err(err.to_string())));
                Message::StateLoaded {
                    path,
                    status,
                    remotes,
                }
            },
            std::convert::identity,
        )
    }

    /// Handles a message; the toast, if any, is for the application to show.
    pub fn update(
        &mut self,
        message: Message,
        auth: &AuthManager,
    ) -> (Task<Message>, Option<Toast>) {
        let Some(repo) = self.repo.clone() else {
            return (Task::none(), None);
        };
        match message {
            Message::Fetch(remote) => {
                self.menu_open = false;
                self.start_action(auth, RemoteAction::Fetch, remote)
            }
            Message::Pull => self.start_action(auth, RemoteAction::Pull, None),
            Message::Push => self.start_action(auth, RemoteAction::Push, None),
            Message::OpenFolder => (
                Task::none(),
                open_folder(repo.path())
                    .err()
                    .map(|err| Toast::error(t("open_folder_failed"), err)),
            ),
            Message::OpenTerminal => (
                Task::none(),
                open_terminal(repo.path())
                    .err()
                    .map(|err| Toast::error(t("open_terminal_failed"), err)),
            ),
            Message::RemotesMenu(open) => {
                self.menu_open = open;
                (Task::none(), None)
            }
            Message::SelectTab(_) => (Task::none(), None),
            Message::StateLoaded {
                path,
                status,
                remotes,
            } => {
                if *path != *repo.path() {
                    return (Task::none(), None);
                }
                self.loading = false;
                let mut error = None;
                self.status = status.map_err(|err| error = Some(err)).ok();
                self.remotes = remotes
                    .map_err(|err| {
                        error.get_or_insert(err);
                    })
                    .unwrap_or_default();
                let toast = error.map(|err| Toast::error(t("header_status_failed"), err));
                (Task::none(), toast)
            }
            Message::ActionFinished {
                path,
                action,
                result,
            } => {
                let here = *path == *repo.path();
                if here {
                    self.running = None;
                }
                // A result for a repository that is no longer open says which one it was.
                let located = |text: String| {
                    if here {
                        text
                    } else {
                        format!("{path}: {text}")
                    }
                };
                match result {
                    Ok(_) if action == RemoteAction::AutoFetch => {
                        (self.reload_if(here, &repo), None)
                    }
                    Ok(done) => (
                        self.reload_if(here, &repo),
                        Some(Toast::success(located(done))),
                    ),
                    Err(_) if action == RemoteAction::AutoFetch && !here => (Task::none(), None),
                    Err(err) => (
                        Task::none(),
                        Some(Toast::error(action.failed(), located(err))),
                    ),
                }
            }
        }
    }

    fn reload_if(&mut self, here: bool, repo: &RepoContext) -> Task<Message> {
        if here {
            self.load(repo)
        } else {
            Task::none()
        }
    }

    /// Starts a fetch for the auto-fetch timer. Does nothing, and says nothing, while
    /// something runs, before the state of `repo` is read, or when it has no remote.
    pub fn auto_fetch(&mut self, repo: &RepoContext, auth: &AuthManager) -> Task<Message> {
        if self.is_busy()
            || self.repo.as_ref().map(RepoContext::path) != Some(repo.path())
            || self.resolve_remote_selection(false, None).is_err()
        {
            return Task::none();
        }
        self.start_action(auth, RemoteAction::AutoFetch, None).0
    }

    /// Runs fetch, pull or push off the UI thread. The token is resolved inside the task:
    /// reading the keyring and refreshing an OAuth token can block.
    fn start_action(
        &mut self,
        auth: &AuthManager,
        action: RemoteAction,
        remote: Option<String>,
    ) -> (Task<Message>, Option<Toast>) {
        let Some(repo) = &self.repo else {
            return (Task::none(), None);
        };
        if self.is_busy() {
            return (Task::none(), None);
        }
        let selection = match self.resolve_remote_selection(action == RemoteAction::Push, remote) {
            Ok(selection) => selection,
            Err(err) => return (Task::none(), Some(Toast::error(action.failed(), err))),
        };
        self.running = Some(action);

        let path = repo.path_arc();
        let auth = auth.clone();
        let network = self.network.clone();
        let task = Task::perform(
            async move {
                let repo_path = Arc::clone(&path);
                let result = tokio::task::spawn_blocking(move || {
                    let token = selection
                        .url
                        .as_deref()
                        .and_then(|url| auth.resolve_for_url(url));
                    run_remote_action(&repo_path, action, &selection, &network, token)
                })
                .await
                .unwrap_or_else(|err| Err(err.to_string()));
                Message::ActionFinished {
                    path,
                    action,
                    result,
                }
            },
            std::convert::identity,
        );
        (task, None)
    }

    /// The remote and branches to use: `remote` when given, else the upstream's remote, else
    /// the first one. `push` picks the remote's push URL for the token lookup.
    fn resolve_remote_selection(
        &self,
        push: bool,
        remote: Option<String>,
    ) -> Result<RemoteSelection, String> {
        let status = self.status.clone().unwrap_or_default();
        let upstream = status.upstream.as_deref().and_then(split_upstream);
        let (remote_name, upstream_branch) = match (remote, upstream) {
            (Some(remote), _) => (remote, None),
            (None, Some((remote, branch))) => (remote.to_string(), Some(branch.to_string())),
            (None, None) => {
                let remote = self
                    .remotes
                    .first()
                    .map(|r| r.name.clone())
                    .ok_or_else(|| t("header_no_remote").to_string())?;
                (remote, None)
            }
        };

        let url = self
            .remotes
            .iter()
            .find(|r| r.name == remote_name)
            .map(|remote| match &remote.push_url {
                Some(push_url) if push => push_url.clone(),
                _ => remote.url.clone(),
            })
            .filter(|url| url != "(no url)");

        Ok(RemoteSelection {
            remote_name,
            url,
            branch: status.branch,
            upstream_branch,
        })
    }

    pub fn view(&self, tab: RepoTab, counts: TabCounts) -> Element<'_, Message> {
        let Some(repo) = &self.repo else {
            return Space::new().into();
        };
        let typo = Typography::current();

        let mut top = row![text(repo.name.clone()).size(typo.sz(17)).font(typo.bold)]
            .spacing(10)
            .align_y(Alignment::Center);
        if let Some(status) = &self.status {
            let branch = match (&status.branch, &status.detached_at) {
                (Some(branch), _) => Some(branch.clone()),
                (None, Some(hash)) => Some(tf("header_detached", &[("hash", hash)])),
                (None, None) => None,
            };
            if let Some(branch) = branch {
                let tip = match &status.upstream {
                    Some(upstream) => tf("header_upstream", &[("upstream", upstream)]),
                    None => t("header_no_upstream").to_string(),
                };
                top = top.push(button::with_tip(chip(&branch, Tone::Neutral), &tip));
            }
            if let (Some(_), Some(ahead), Some(behind)) =
                (&status.upstream, status.ahead, status.behind)
            {
                let tone = if behind > 0 {
                    Tone::Warning
                } else {
                    Tone::Success
                };
                top = top.push(chip(&format!("↑{ahead} ↓{behind}"), tone));
            }
        }
        top = top
            .push(button::icon(
                FOLDER_GLYPH,
                Some(Message::OpenFolder),
                t("sidebar_open_folder"),
            ))
            .push(button::icon(
                TERMINAL_GLYPH,
                Some(Message::OpenTerminal),
                t("sidebar_open_terminal"),
            ))
            .push(Space::new().width(Length::Fill));

        let primary = self.status.as_ref().and_then(|status| {
            primary_action(status.ahead.unwrap_or(0), status.behind.unwrap_or(0))
        });
        let actions = [
            (RemoteAction::Fetch, Message::Fetch(None)),
            (RemoteAction::Pull, Message::Pull),
            (RemoteAction::Push, Message::Push),
        ];
        let mut buttons = row![].spacing(6);
        for (action, message) in actions {
            let running = match self.running {
                Some(RemoteAction::AutoFetch) => action == RemoteAction::Fetch,
                running => running == Some(action),
            };
            let label = if running {
                action.progress()
            } else {
                action.label()
            };
            let press = (!self.is_busy()).then_some(message);
            buttons = buttons.push(if primary == Some(action) {
                button::primary(label, press)
            } else {
                button::secondary(label, press)
            });
        }
        top = top.push(buttons).push(menu::popup(
            button::icon(
                MORE_GLYPH,
                Some(Message::RemotesMenu(!self.menu_open)),
                t("header_remotes"),
            ),
            self.menu_open,
            self.remote_entries(),
            Message::RemotesMenu(false),
        ));

        let items: Vec<(String, Option<usize>)> = RepoTab::ALL
            .iter()
            .map(|&each| {
                let count = match each {
                    RepoTab::Changes => counts.changes,
                    RepoTab::Stashes => counts.stashes,
                    RepoTab::History | RepoTab::Branches => 0,
                };
                (each.label().to_string(), (count > 0).then_some(count))
            })
            .collect();
        let selected = RepoTab::ALL
            .iter()
            .position(|&each| each == tab)
            .unwrap_or(0);
        let strip = tabs(&items, selected, |index| {
            Message::SelectTab(RepoTab::ALL[index])
        });

        let divider =
            container(Space::new().width(Length::Fill).height(1)).style(|_| container::Style {
                background: Some(Palette::DIVIDER().into()),
                ..Default::default()
            });

        column![
            container(top).padding(Padding {
                top: 12.0,
                right: 18.0,
                bottom: 8.0,
                left: 18.0,
            }),
            container(strip).padding(Padding {
                top: 0.0,
                right: 18.0,
                bottom: 8.0,
                left: 18.0,
            }),
            divider,
        ]
        .into()
    }

    /// One "Fetch <remote>" entry per remote, with its URL on the right.
    fn remote_entries(&self) -> Vec<Entry<Message>> {
        if self.remotes.is_empty() {
            return vec![Entry::Item(MenuItem {
                label: t("header_no_remotes").to_string(),
                shortcut: None,
                on_select: Message::RemotesMenu(false),
                danger: false,
            })];
        }
        self.remotes
            .iter()
            .map(|remote| {
                Entry::Item(MenuItem {
                    label: tf("header_fetch_remote", &[("remote", &remote.name)]),
                    shortcut: Some(remote.url.clone()),
                    on_select: Message::Fetch(Some(remote.name.clone())),
                    danger: false,
                })
            })
            .collect()
    }
}

/// Opens a terminal in the folder `path`, without waiting for it to close.
pub fn open_terminal(path: &str) -> Result<(), String> {
    let dir = Path::new(path);
    if !dir.is_dir() {
        return Err(tf("header_folder_missing", &[("path", path)]));
    }

    let mut error = None;
    for mut command in terminal_commands(dir) {
        match command.spawn() {
            Ok(_) => return Ok(()),
            Err(err) => error = Some(err),
        }
    }
    Err(match error {
        Some(err) if err.kind() != ErrorKind::NotFound => err.to_string(),
        _ => t("header_no_terminal").to_string(),
    })
}

/// Opens a folder in the system file manager, without waiting for it to close.
pub fn open_folder(path: &str) -> Result<(), String> {
    open::that_detached(path).map_err(|err| err.to_string())
}

/// Runs fetch, pull or push. Blocking: call it off the UI thread.
fn run_remote_action(
    path: &str,
    action: RemoteAction,
    selection: &RemoteSelection,
    network: &NetworkOptions,
    token: Option<String>,
) -> Result<String, String> {
    let remote = &selection.remote_name;
    if matches!(action, RemoteAction::Fetch | RemoteAction::AutoFetch) {
        fetch_remote(path, remote, network, token).map_err(|err| err.to_string())?;
        return Ok(tf("header_fetched", &[("remote", remote)]));
    }
    let branch = selection
        .branch
        .as_deref()
        .ok_or_else(|| t("header_no_branch").to_string())?;
    let upstream = selection.upstream_branch.as_deref().unwrap_or(branch);
    let names = [("branch", branch), ("remote", remote.as_str())];
    if action == RemoteAction::Pull {
        let outcome = pull_branch(path, remote, branch, upstream, network, token)
            .map_err(|err| err.to_string())?;
        return Ok(match outcome {
            PullOutcome::UpToDate => t("header_up_to_date").to_string(),
            PullOutcome::FastForward => tf("header_pulled", &names),
        });
    }
    push_branch(path, remote, branch, upstream, network, token).map_err(|err| err.to_string())?;
    Ok(tf("header_pushed", &names))
}

/// The terminal emulators to try, in order, each opening in `path`.
///
/// The path is only ever the working directory or a whole argument, never part of a shell
/// command line, so characters such as `&`, `'` or spaces in it cannot run anything.
fn terminal_commands(path: &Path) -> Vec<ProcessCommand> {
    #[cfg(target_os = "windows")]
    let candidates: Vec<(&str, Vec<&OsStr>)> = vec![("cmd", Vec::new())];

    #[cfg(target_os = "macos")]
    let candidates: Vec<(&str, Vec<&OsStr>)> = vec![(
        "open",
        vec!["-a".as_ref(), "Terminal".as_ref(), path.as_os_str()],
    )];

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let dir = path.as_os_str();
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let candidates: Vec<(&str, Vec<&OsStr>)> = vec![
        ("x-terminal-emulator", Vec::new()),
        ("gnome-terminal", vec!["--working-directory".as_ref(), dir]),
        ("konsole", vec!["--workdir".as_ref(), dir]),
        ("xfce4-terminal", vec!["--working-directory".as_ref(), dir]),
        ("xterm", Vec::new()),
        ("alacritty", vec!["--working-directory".as_ref(), dir]),
        ("kitty", vec!["--directory".as_ref(), dir]),
        ("wezterm", vec!["start".as_ref(), "--cwd".as_ref(), dir]),
    ];

    candidates
        .into_iter()
        .map(|(program, args)| {
            let mut command = ProcessCommand::new(program);
            command.args(args).current_dir(path);
            command
        })
        .collect()
}

/// Splits an upstream "remote/branch" into its two parts.
fn split_upstream(upstream: &str) -> Option<(&str, &str)> {
    upstream.split_once('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_action_follows_divergence() {
        assert_eq!(primary_action(2, 0), Some(RemoteAction::Push));
        assert_eq!(primary_action(0, 1), Some(RemoteAction::Pull));
        assert_eq!(primary_action(0, 0), None);
        assert_eq!(primary_action(2, 1), Some(RemoteAction::Push));
    }

    #[test]
    fn terminal_commands_never_put_the_path_in_a_command_line() {
        for path in ["/tmp/R&D/it's here", "/tmp/a;b $(touch x)", r"C:\R&D\x y"] {
            let path = Path::new(path);
            let commands = terminal_commands(path);
            assert!(!commands.is_empty());
            for command in &commands {
                assert_eq!(command.get_current_dir(), Some(path));
                for arg in command.get_args() {
                    assert!(
                        arg == path.as_os_str()
                            || !arg.to_string_lossy().contains(&*path.to_string_lossy()),
                        "{:?} embeds the path in {arg:?}",
                        command.get_program()
                    );
                }
            }
        }
    }
}
