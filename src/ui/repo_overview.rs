// Panneau de vue d'ensemble du dépôt — migration Iced 0.13
// Affiche le statut, les remotes, les actions rapides et le contexte d'auto-fetch.

use std::process::Command as ProcessCommand;

use iced::widget::{button, container, horizontal_rule, text, Column, Row};
use iced::{Element, Length, Task};

use crate::ui::theme::Theme;

use crate::auth::AuthManager;
use crate::config::NetworkOptions;
use crate::git::remote::{
    PullOutcome, RemoteInfo, fetch_remote, list_remotes, pull_branch, push_branch,
};
use crate::git::status::{RepoStatus, read_repo_status};
use crate::ui::context::RepoContext;

// ── Structures auxiliaires ─────────────────────────────────────────────────

/// Contexte nécessaire pour l'auto-fetch en arrière-plan.
#[derive(Debug, Clone)]
pub struct AutoFetchContext {
    pub repo_path: String,
    pub remote_name: String,
    pub token: Option<String>,
    pub network: NetworkOptions,
}

/// Sélection résolue d'un remote et d'une branche.
#[derive(Debug, Clone)]
struct RemoteSelection {
    remote_name: String,
    branch: Option<String>,
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
    /// Résultat d'un fetch (succès ou erreur).
    FetchResult(Result<String, String>),
    /// Résultat d'un pull (succès ou erreur).
    PullResult(Result<String, String>),
    /// Résultat d'un push (succès ou erreur).
    PushResult(Result<String, String>),
}

// ── Panneau principal ──────────────────────────────────────────────────────

pub struct RepoOverviewPanel {
    status: Option<RepoStatus>,
    remotes: Vec<RemoteInfo>,
    last_repo: Option<String>,
    error: Option<String>,
    action_status: Option<String>,
    network: NetworkOptions,
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
        }
    }

    /// Met à jour les préférences réseau.
    pub fn set_network_preferences(&mut self, network: NetworkOptions) {
        self.network = network;
    }

    /// Définit un message de statut d'action.
    pub fn set_action_status<S: Into<String>>(&mut self, status: Option<S>) {
        self.action_status = status.map(Into::into);
    }

    /// Traite un message et retourne une commande Iced.
    pub fn update(
        &mut self,
        message: Message,
        repo: Option<&RepoContext>,
        auth: &AuthManager,
    ) -> Task<Message> {
        match message {
            Message::Fetch => {
                if let Some(repo) = repo {
                    let result = self.do_fetch(repo, auth);
                    if result.is_ok() {
                        self.reload_repo_state(repo);
                    }
                    self.action_status = Some(match result {
                        Ok(msg) => msg,
                        Err(err) => format!("Fetch failed: {err}"),
                    });
                }
            }
            Message::Pull => {
                if let Some(repo) = repo {
                    let result = self.do_pull(repo, auth);
                    if result.is_ok() {
                        self.reload_repo_state(repo);
                    }
                    self.action_status = Some(match result {
                        Ok(msg) => msg,
                        Err(err) => format!("Pull failed: {err}"),
                    });
                }
            }
            Message::Push => {
                if let Some(repo) = repo {
                    let result = self.do_push(repo, auth);
                    if result.is_ok() {
                        self.reload_repo_state(repo);
                    }
                    self.action_status = Some(match result {
                        Ok(msg) => msg,
                        Err(err) => format!("Push failed: {err}"),
                    });
                }
            }
            Message::OpenTerminal => {
                if let Some(repo) = repo {
                    match self.open_terminal(repo) {
                        Ok(msg) => self.action_status = Some(msg),
                        Err(err) => self.action_status = Some(format!("Terminal failed: {err}")),
                    }
                }
            }
            Message::OpenFileExplorer => {
                if let Some(repo) = repo {
                    match self.open_file_explorer(repo) {
                        Ok(msg) => self.action_status = Some(msg),
                        Err(err) => {
                            self.action_status = Some(format!("File explorer failed: {err}"))
                        }
                    }
                }
            }
            // Résultats asynchrones (pour usage futur si les opérations deviennent async)
            Message::FetchResult(result) => {
                self.action_status = Some(match result {
                    Ok(msg) => msg,
                    Err(err) => format!("Fetch failed: {err}"),
                });
            }
            Message::PullResult(result) => {
                self.action_status = Some(match result {
                    Ok(msg) => msg,
                    Err(err) => format!("Pull failed: {err}"),
                });
            }
            Message::PushResult(result) => {
                self.action_status = Some(match result {
                    Ok(msg) => msg,
                    Err(err) => format!("Push failed: {err}"),
                });
            }
        }

        Task::none()
    }

    /// Construit la vue du panneau.
    pub fn view<'a>(
        &'a self,
        _theme: &'a Theme,
        repo: Option<&'a RepoContext>,
    ) -> Element<'a, Message> {
        let mut content = Column::new()
            .spacing(8)
            .padding(10)
            .width(Length::Fill);

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
        let branch = status
            .branch
            .unwrap_or_else(|| "(detached)".to_string());
        let upstream = status
            .upstream
            .unwrap_or_else(|| "No upstream".to_string());
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

        col = col.push(
            container(branch_row)
                .width(Length::Fill)
                .padding(10),
        );

        col.into()
    }

    /// Section listant les remotes configurés.
    fn build_remotes_section(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(4);

        col = col.push(text("Remotes").size(16));

        if self.remotes.is_empty() {
            col = col.push(
                text("No remotes configured for this repository.").size(12),
            );
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

        let actions_row = Row::new()
            .spacing(8)
            .push(
                button(text("Fetch").size(13))
                    .on_press(Message::Fetch)
                    .padding(6),
            )
            .push(
                button(text("Pull").size(13))
                    .on_press(Message::Pull)
                    .padding(6),
            )
            .push(
                button(text("Push").size(13))
                    .on_press(Message::Push)
                    .padding(6),
            )
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
    pub fn refresh(&mut self, repo: &RepoContext) {
        if self.last_repo.as_deref() == Some(repo.path()) {
            return;
        }
        self.action_status = None;
        self.reload_repo_state(repo);
    }

    /// Recharge complètement l'état du dépôt (statut + remotes).
    pub fn reload_repo_state(&mut self, repo: &RepoContext) {
        self.last_repo = Some(repo.path().to_string());
        self.status = None;
        self.remotes.clear();
        self.error = None;

        match read_repo_status(repo.path()) {
            Ok(status) => self.status = Some(status),
            Err(err) => {
                self.error = Some(format!("Failed to read repository status: {err}"));
            }
        }

        match list_remotes(repo.path()) {
            Ok(remotes) => self.remotes = remotes,
            Err(err) => {
                self.error
                    .get_or_insert_with(|| format!("Failed to read remotes: {err}"));
            }
        }
    }

    // ── Contexte d'auto-fetch ──────────────────────────────────────────

    /// Construit le contexte pour l'auto-fetch en arrière-plan.
    pub fn auto_fetch_context(
        &mut self,
        repo: &RepoContext,
        auth: &AuthManager,
    ) -> Result<AutoFetchContext, String> {
        if self.last_repo.as_deref() != Some(repo.path()) {
            self.reload_repo_state(repo);
        }
        let selection = self.resolve_remote_selection()?;
        let token = self.resolve_remote_token(auth, &selection.remote_name);
        Ok(AutoFetchContext {
            repo_path: repo.path().to_string(),
            remote_name: selection.remote_name,
            token,
            network: self.network.clone(),
        })
    }

    // ── Opérations git ─────────────────────────────────────────────────

    /// Effectue un fetch sur le remote résolu.
    fn do_fetch(&self, repo: &RepoContext, auth: &AuthManager) -> Result<String, String> {
        let selection = self.resolve_remote_selection()?;
        let token = self.resolve_remote_token(auth, &selection.remote_name);
        fetch_remote(repo.path(), &selection.remote_name, &self.network, token)
            .map_err(|err| err.to_string())?;
        Ok(format!("Fetched {}", selection.remote_name))
    }

    /// Effectue un pull sur la branche courante.
    fn do_pull(&self, repo: &RepoContext, auth: &AuthManager) -> Result<String, String> {
        let selection = self.resolve_remote_selection()?;
        let branch = selection
            .branch
            .ok_or_else(|| "No branch checked out for pull.".to_string())?;
        let token = self.resolve_remote_token(auth, &selection.remote_name);
        let outcome = pull_branch(
            repo.path(),
            &selection.remote_name,
            &branch,
            &self.network,
            token,
        )
        .map_err(|err| err.to_string())?;
        let message = match outcome {
            PullOutcome::UpToDate => "Already up to date.".to_string(),
            PullOutcome::FastForward => {
                format!("Pulled {} from {}", branch, selection.remote_name)
            }
        };
        Ok(message)
    }

    /// Effectue un push de la branche courante.
    fn do_push(&self, repo: &RepoContext, auth: &AuthManager) -> Result<String, String> {
        let selection = self.resolve_remote_selection()?;
        let branch = selection
            .branch
            .ok_or_else(|| "No branch checked out for push.".to_string())?;
        let token = self.resolve_remote_token(auth, &selection.remote_name);
        push_branch(
            repo.path(),
            &selection.remote_name,
            &branch,
            &self.network,
            token,
        )
        .map_err(|err| err.to_string())?;
        Ok(format!("Pushed {} to {}", branch, selection.remote_name))
    }

    /// Résout le remote et la branche à utiliser pour les opérations réseau.
    fn resolve_remote_selection(&self) -> Result<RemoteSelection, String> {
        let status = self.status.clone().unwrap_or_default();
        let upstream = status
            .upstream
            .as_deref()
            .and_then(split_upstream);
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

        let branch = upstream_branch.or(status.branch);
        Ok(RemoteSelection { remote_name, branch })
    }

    /// Résout le token d'authentification pour un remote donné.
    fn resolve_remote_token(&self, auth: &AuthManager, remote_name: &str) -> Option<String> {
        let remote = self
            .remotes
            .iter()
            .find(|r| r.name == remote_name)?;
        if remote.url == "(no url)" {
            return None;
        }
        auth.resolve_for_url(&remote.url)
            .or_else(|| auth.resolve_for_host(&remote.url))
    }

    // ── Actions système ────────────────────────────────────────────────

    /// Ouvre un terminal dans le répertoire du dépôt.
    fn open_terminal(&self, repo: &RepoContext) -> Result<String, String> {
        #[cfg(target_os = "windows")]
        {
            ProcessCommand::new("cmd")
                .args(["/K", "cd", "/d", repo.path()])
                .spawn()
                .map_err(|err| err.to_string())?;
            return Ok("Terminal opened".to_string());
        }

        #[cfg(target_os = "macos")]
        {
            ProcessCommand::new("open")
                .args(["-a", "Terminal", repo.path()])
                .spawn()
                .map_err(|err| err.to_string())?;
            return Ok("Terminal opened".to_string());
        }

        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        {
            let xterm_command = format!("cd '{}' && exec bash", repo.path());
            let candidates: Vec<(&str, Vec<String>)> = vec![
                ("x-terminal-emulator", Vec::new()),
                (
                    "gnome-terminal",
                    vec!["--working-directory".into(), repo.path().to_string()],
                ),
                ("konsole", vec!["--workdir".into(), repo.path().to_string()]),
                (
                    "xfce4-terminal",
                    vec!["--working-directory".into(), repo.path().to_string()],
                ),
                (
                    "xterm",
                    vec!["-e".into(), "bash".into(), "-lc".into(), xterm_command],
                ),
                (
                    "alacritty",
                    vec!["--working-directory".into(), repo.path().to_string()],
                ),
                ("kitty", vec!["--directory".into(), repo.path().to_string()]),
                (
                    "wezterm",
                    vec!["start".into(), "--cwd".into(), repo.path().to_string()],
                ),
            ];

            for (terminal, args) in candidates {
                let mut command = ProcessCommand::new(terminal);
                command.args(args);
                command.current_dir(repo.path());
                if command.spawn().is_ok() {
                    return Ok("Terminal opened".to_string());
                }
            }

            Err("No supported terminal emulator found on PATH".to_string())
        }
    }

    /// Ouvre l'explorateur de fichiers dans le répertoire du dépôt.
    fn open_file_explorer(&self, repo: &RepoContext) -> Result<String, String> {
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut cmd = ProcessCommand::new("explorer");
            cmd.arg(repo.path());
            cmd
        };

        #[cfg(target_os = "macos")]
        let mut command = {
            let mut cmd = ProcessCommand::new("open");
            cmd.arg(repo.path());
            cmd
        };

        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        let mut command = {
            let mut cmd = ProcessCommand::new("xdg-open");
            cmd.arg(repo.path());
            cmd
        };

        command.spawn().map_err(|err| err.to_string())?;
        Ok("File explorer opened".to_string())
    }
}

// ── Fonctions utilitaires ──────────────────────────────────────────────────

/// Sépare un upstream "remote/branch" en ses deux composants.
fn split_upstream(upstream: &str) -> Option<(&str, &str)> {
    let mut parts = upstream.splitn(2, '/');
    let remote = parts.next()?;
    let branch = parts.next()?;
    Some((remote, branch))
}
