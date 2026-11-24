// Panneau de vue d'ensemble du dépôt — migration Iced 0.13
// Affiche le statut, les remotes, les actions rapides et le contexte d'auto-fetch.

use std::ffi::OsStr;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command as ProcessCommand;
use std::sync::Arc;

use iced::widget::{button, container, horizontal_rule, text, Column, Row};
use iced::{Element, Length, Task};

use crate::ui::theme::Theme;

use crate::auth::AuthManager;
use crate::config::NetworkOptions;
use crate::git::remote::{
    fetch_remote, list_remotes, pull_branch, push_branch, PullOutcome, RemoteInfo,
};
use crate::git::status::{read_repo_status, RepoStatus};
use crate::ui::context::RepoContext;

// ── Structures auxiliaires ─────────────────────────────────────────────────

/// Opération réseau lancée sur le remote du dépôt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteAction {
    Fetch,
    Pull,
    Push,
}

impl RemoteAction {
    fn label(self) -> &'static str {
        match self {
            Self::Fetch => "Fetch",
            Self::Pull => "Pull",
            Self::Push => "Push",
        }
    }
}

/// Sélection résolue d'un remote et d'une branche.
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

// ── Messages du panneau ────────────────────────────────────────────────────

/// Messages émis par le panneau de vue d'ensemble.
#[derive(Debug, Clone)]
pub enum Message {
    /// Lancer un fetch.
    Fetch,
    /// Lancer un pull.
    Pull,
    /// Lancer un push.
    Push,
    /// Ouvrir un terminal dans le dépôt.
    OpenTerminal,
    /// Ouvrir l'explorateur de fichiers dans le dépôt.
    OpenFileExplorer,
    /// Statut et remotes lus pour le dépôt `path`.
    StateLoaded {
        path: Arc<str>,
        status: Result<RepoStatus, String>,
        remotes: Result<Vec<RemoteInfo>, String>,
    },
    /// Fin d'une opération réseau sur le dépôt `path`. `auto` marque un auto-fetch.
    ActionFinished {
        path: Arc<str>,
        action: RemoteAction,
        auto: bool,
        result: Result<String, String>,
    },
}

// ── Panneau principal ──────────────────────────────────────────────────────

pub struct RepoOverviewPanel {
    status: Option<RepoStatus>,
    remotes: Vec<RemoteInfo>,
    last_repo: Option<String>,
    error: Option<String>,
    action_status: Option<String>,
    network: NetworkOptions,
    /// A state load or a network operation is running for `last_repo`.
    busy: bool,
}

impl RepoOverviewPanel {
    /// Crée un nouveau panneau de vue d'ensemble.
    pub fn new(network: NetworkOptions) -> Self {
        Self {
            status: None,
            remotes: Vec::new(),
            last_repo: None,
            error: None,
            action_status: None,
            network,
            busy: false,
        }
    }

    /// Met à jour les préférences réseau.
    pub fn set_network_preferences(&mut self, network: NetworkOptions) {
        self.network = network;
    }

    /// Vrai pendant un chargement d'état ou une opération réseau sur le dépôt courant.
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// Traite un message et retourne une commande Iced.
    pub fn update(
        &mut self,
        message: Message,
        repo: Option<&RepoContext>,
        auth: &AuthManager,
    ) -> Task<Message> {
        let Some(repo) = repo else {
            return Task::none();
        };
        match message {
            Message::Fetch => self.start_action(repo, auth, RemoteAction::Fetch, false),
            Message::Pull => self.start_action(repo, auth, RemoteAction::Pull, false),
            Message::Push => self.start_action(repo, auth, RemoteAction::Push, false),
            Message::OpenTerminal => {
                self.action_status = Some(match self.open_terminal(repo) {
                    Ok(msg) => msg,
                    Err(err) => format!("Terminal failed: {err}"),
                });
                Task::none()
            }
            Message::OpenFileExplorer => {
                self.action_status = Some(match open_folder(repo.path()) {
                    Ok(()) => "File explorer opened".to_string(),
                    Err(err) => format!("File explorer failed: {err}"),
                });
                Task::none()
            }
            Message::StateLoaded {
                path,
                status,
                remotes,
            } => {
                if *path != *repo.path() {
                    return Task::none();
                }
                self.busy = false;
                self.error = None;
                self.status = status
                    .map_err(|err| {
                        self.error = Some(format!("Failed to read repository status: {err}"));
                    })
                    .ok();
                self.remotes = remotes
                    .map_err(|err| {
                        self.error
                            .get_or_insert_with(|| format!("Failed to read remotes: {err}"));
                    })
                    .unwrap_or_default();
                Task::none()
            }
            Message::ActionFinished {
                path,
                action,
                auto,
                result,
            } => {
                if *path != *repo.path() {
                    return Task::none();
                }
                self.busy = false;
                let prefix = if auto { "Auto-fetch" } else { action.label() };
                match result {
                    Ok(msg) => {
                        self.action_status = Some(if auto {
                            format!("{prefix}: {msg}")
                        } else {
                            msg
                        });
                        self.reload_repo_state(repo)
                    }
                    Err(err) => {
                        self.action_status = Some(format!("{prefix} failed: {err}"));
                        Task::none()
                    }
                }
            }
        }
    }

    /// Construit la vue du panneau.
    pub fn view<'a>(
        &'a self,
        _theme: &'a Theme,
        repo: Option<&'a RepoContext>,
    ) -> Element<'a, Message> {
        let mut content = Column::new().spacing(8).padding(10).width(Length::Fill);

        // ── En-tête + statut d'action ───────────────────────────────────
        let mut header = Row::new().spacing(12);
        header = header.push(text("Repository overview").size(20));
        if let Some(action_status) = &self.action_status {
            header = header.push(text(action_status.clone()).size(13));
        }
        content = content.push(header);

        // Vérifier si un dépôt est ouvert
        let repo = match repo {
            Some(r) => r,
            None => {
                content = content.push(
                    text("Select or clone a repository to see its Git status, remotes, and quick actions.")
                        .size(13),
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
        }

        // ── Résumé : nom + chemin ───────────────────────────────────────
        content = content.push(self.build_summary(repo));
        content = content.push(horizontal_rule(1));

        // ── Section branche ─────────────────────────────────────────────
        content = content.push(self.build_branch_section());
        content = content.push(horizontal_rule(1));

        // ── Section remotes ─────────────────────────────────────────────
        content = content.push(self.build_remotes_section());
        content = content.push(horizontal_rule(1));

        // ── Actions rapides ─────────────────────────────────────────────
        content = content.push(self.build_quick_actions());

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    // ── Construction des sous-vues ─────────────────────────────────────

    /// Résumé du dépôt : nom et chemin.
    fn build_summary<'a>(&'a self, repo: &'a RepoContext) -> Element<'a, Message> {
        let col = Column::new()
            .spacing(2)
            .push(text(&repo.name).size(18))
            .push(text(repo.path()).size(12));
        col.into()
    }

    /// Section de la branche courante avec upstream et ahead/behind.
    fn build_branch_section(&self) -> Element<'_, Message> {
        let status = self.status.clone().unwrap_or_default();
        let branch = status.branch.unwrap_or_else(|| "(detached)".to_string());
        let upstream = status.upstream.unwrap_or_else(|| "No upstream".to_string());
        let ahead = status.ahead.unwrap_or(0);
        let behind = status.behind.unwrap_or(0);

        let mut col = Column::new().spacing(4).padding(8);

        col = col.push(text("Branch").size(16));

        // Informations de la branche
        let branch_info = Column::new()
            .spacing(2)
            .push(text(branch).size(14))
            .push(text(format!("Upstream: {upstream}")).size(12));

        // Statistiques ahead/behind
        let stats = Row::new()
            .spacing(12)
            .push(text(format!("Ahead: {ahead}")).size(13))
            .push(text(format!("Behind: {behind}")).size(13));

        let branch_row = Row::new()
            .spacing(16)
            .width(Length::Fill)
            .push(branch_info)
            .push(stats);

        col = col.push(container(branch_row).width(Length::Fill).padding(10));

        col.into()
    }

    /// Section listant les remotes configurés.
    fn build_remotes_section(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(4);

        col = col.push(text("Remotes").size(16));

        if self.remotes.is_empty() {
            col = col.push(text("No remotes configured for this repository.").size(12));
            return col.into();
        }

        for remote in &self.remotes {
            let card = container(
                Row::new()
                    .spacing(8)
                    .push(text(&remote.name).size(13))
                    .push(text(&remote.url).size(12)),
            )
            .width(Length::Fill)
            .padding(8);

            col = col.push(card);
        }

        col.into()
    }

    /// Ligne de boutons d'actions rapides.
    fn build_quick_actions(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(4);

        col = col.push(text("Quick actions").size(16));

        let network_button = |label, message| {
            button(text(label).size(13))
                .on_press_maybe((!self.busy).then_some(message))
                .padding(6)
        };

        let actions_row = Row::new()
            .spacing(8)
            .push(network_button("Fetch", Message::Fetch))
            .push(network_button("Pull", Message::Pull))
            .push(network_button("Push", Message::Push))
            .push(
                button(text("Open terminal").size(13))
                    .on_press(Message::OpenTerminal)
                    .padding(6),
            )
            .push(
                button(text("Open file explorer").size(13))
                    .on_press(Message::OpenFileExplorer)
                    .padding(6),
            );

        col = col.push(actions_row);
        col.into()
    }

    // ── Rafraîchissement de l'état ─────────────────────────────────────

    /// Rafraîchit l'état du dépôt si le chemin a changé.
    pub fn refresh(&mut self, repo: &RepoContext) -> Task<Message> {
        if self.last_repo.as_deref() == Some(repo.path()) {
            return Task::none();
        }
        self.action_status = None;
        self.reload_repo_state(repo)
    }

    /// Relit le statut et les remotes du dépôt hors du thread UI.
    pub fn reload_repo_state(&mut self, repo: &RepoContext) -> Task<Message> {
        if self.last_repo.as_deref() != Some(repo.path()) {
            self.last_repo = Some(repo.path().to_string());
            self.status = None;
            self.remotes.clear();
            self.error = None;
        }
        self.busy = true;

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

    // ── Opérations git ─────────────────────────────────────────────────

    /// Lance un fetch en arrière-plan pour l'auto-fetch. Ne fait rien, sans message, si une
    /// opération est en cours, si l'état du dépôt n'est pas encore lu ou s'il n'a pas de remote.
    pub fn auto_fetch(&mut self, repo: &RepoContext, auth: &AuthManager) -> Task<Message> {
        if self.busy
            || self.last_repo.as_deref() != Some(repo.path())
            || self.resolve_remote_selection(false).is_err()
        {
            return Task::none();
        }
        self.start_action(repo, auth, RemoteAction::Fetch, true)
    }

    /// Lance fetch, pull ou push hors du thread UI. Le token est résolu dans la tâche :
    /// la lecture du trousseau et le rafraîchissement OAuth peuvent bloquer.
    fn start_action(
        &mut self,
        repo: &RepoContext,
        auth: &AuthManager,
        action: RemoteAction,
        auto: bool,
    ) -> Task<Message> {
        if self.busy {
            return Task::none();
        }
        let selection = match self.resolve_remote_selection(action == RemoteAction::Push) {
            Ok(selection) => selection,
            Err(err) => {
                self.action_status = Some(format!("{} failed: {err}", action.label()));
                return Task::none();
            }
        };
        self.busy = true;
        self.action_status = Some(format!("{} {}...", action.label(), selection.remote_name));

        let path = repo.path_arc();
        let auth = auth.clone();
        let network = self.network.clone();
        Task::perform(
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
                    auto,
                    result,
                }
            },
            std::convert::identity,
        )
    }

    /// Résout le remote et la branche à utiliser pour les opérations réseau, avec l'URL de
    /// push quand `push` est vrai.
    fn resolve_remote_selection(&self, push: bool) -> Result<RemoteSelection, String> {
        let status = self.status.clone().unwrap_or_default();
        let upstream = status.upstream.as_deref().and_then(split_upstream);
        let (remote_name, upstream_branch) = if let Some((remote, branch)) = upstream {
            (remote.to_string(), Some(branch.to_string()))
        } else {
            let remote = self
                .remotes
                .first()
                .map(|r| r.name.clone())
                .ok_or_else(|| "No remotes configured for this repository.".to_string())?;
            (remote, None)
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

    // ── Actions système ────────────────────────────────────────────────

    /// Ouvre un terminal dans le répertoire du dépôt.
    fn open_terminal(&self, repo: &RepoContext) -> Result<String, String> {
        let path = Path::new(repo.path());
        if !path.is_dir() {
            return Err(format!("Repository folder not found: {}", repo.path()));
        }

        let mut error = None;
        for mut command in terminal_commands(path) {
            match command.spawn() {
                Ok(_) => return Ok("Terminal opened".to_string()),
                Err(err) => error = Some(err),
            }
        }
        Err(match error {
            Some(err) if err.kind() != ErrorKind::NotFound => err.to_string(),
            _ => "No supported terminal emulator found on PATH".to_string(),
        })
    }
}

// ── Fonctions utilitaires ──────────────────────────────────────────────────

/// Ouvre un dossier dans le gestionnaire de fichiers du système, sans attendre sa fermeture.
pub fn open_folder(path: &str) -> Result<(), String> {
    open::that_detached(path).map_err(|err| err.to_string())
}

/// Exécute fetch, pull ou push. Bloquant : à appeler hors du thread UI.
fn run_remote_action(
    path: &str,
    action: RemoteAction,
    selection: &RemoteSelection,
    network: &NetworkOptions,
    token: Option<String>,
) -> Result<String, String> {
    let remote = &selection.remote_name;
    if action == RemoteAction::Fetch {
        fetch_remote(path, remote, network, token).map_err(|err| err.to_string())?;
        return Ok(format!("Fetched {remote}"));
    }
    let branch = selection.branch.as_deref().ok_or_else(|| {
        format!(
            "No branch checked out for {}.",
            action.label().to_lowercase()
        )
    })?;
    let upstream = selection.upstream_branch.as_deref().unwrap_or(branch);
    if action == RemoteAction::Pull {
        let outcome = pull_branch(path, remote, branch, upstream, network, token)
            .map_err(|err| err.to_string())?;
        return Ok(match outcome {
            PullOutcome::UpToDate => "Already up to date.".to_string(),
            PullOutcome::FastForward => format!("Pulled {branch} from {remote}"),
        });
    }
    push_branch(path, remote, branch, upstream, network, token).map_err(|err| err.to_string())?;
    Ok(format!("Pushed {branch} to {remote}"))
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

/// Sépare un upstream "remote/branch" en ses deux composants.
fn split_upstream(upstream: &str) -> Option<(&str, &str)> {
    upstream.split_once('/')
}

#[cfg(test)]
mod tests {
    use super::*;

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
